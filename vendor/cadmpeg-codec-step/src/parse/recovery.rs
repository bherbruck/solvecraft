// SPDX-License-Identifier: Apache-2.0
//! Bounded exchange statement recovery and DATA dependency closure.

use super::{
    BTreeMap, BTreeSet, DecodeContext, ParseDiagnostic, ParseDiagnosticKind, ParseError, Parser,
    Range, RawRecord, Value,
};

impl Parser<'_, '_, '_> {
    pub(super) fn diagnostic(
        &mut self,
        offset: usize,
        kind: ParseDiagnosticKind,
        message: std::fmt::Arguments<'_>,
    ) -> Result<(), ParseError> {
        let message = self
            .budget
            .format_retained(message, "step_recovery_diagnostic_text")?;
        self.budget.push_vec(
            &mut self.diagnostics,
            ParseDiagnostic {
                offset,
                kind,
                message,
            },
            "step_parse_diagnostics",
        )?;
        Ok(())
    }

    fn omit_record(
        &mut self,
        span: Range<usize>,
        omitted: &mut Vec<Range<usize>>,
        reason: std::fmt::Arguments<'_>,
    ) -> Result<(), ParseError> {
        self.diagnostic(span.start, ParseDiagnosticKind::RecordOmitted, reason)?;
        self.budget
            .push_vec(omitted, span, "step_omitted_record_spans")?;
        Ok(())
    }

    pub(super) fn recover_header_record(
        &mut self,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<Option<super::HeaderRecord>, ParseError> {
        let start = self.current_offset();
        let diagnostic_start = self.diagnostics.len();
        let parsed = (|| {
            let name = self.take_name()?;
            let parameters = self.parameter_nesting(Self::parameters_inner)?;
            self.punct(&super::TokenKind::Semicolon)?;
            Ok(super::HeaderRecord {
                name,
                parameters,
                offset: start,
                end: self.previous_end(),
            })
        })();
        match parsed {
            Ok(record) => Ok(Some(record)),
            Err(error @ (ParseError::Resource(_) | ParseError::Lex(_))) => Err(error),
            Err(error @ ParseError::Syntax { .. }) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(
                    self.lexer.input(),
                    start,
                    self.lexer.is_draft(),
                    self.budget,
                )?;
                self.diagnostic(
                    start,
                    ParseDiagnosticKind::HeaderMetadataNoncanonical,
                    format_args!(
                        "bounded HEADER statement omitted: {error}; exact source retained"
                    ),
                )?;
                self.budget
                    .push_vec(omitted, start..end, "step_omitted_record_spans")?;
                self.lexer
                    .set_literal_admission(super::LiteralAdmission::Metadata);
                self.lexer.seek(end);
                self.last_end = end;
                self.current = self.lex_next()?;
                Ok(None)
            }
        }
    }

    pub(super) fn recover_data_record(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        ambiguous: &mut BTreeSet<u64>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        let start = self.current_offset();
        let source_id = self.current.as_ref().and_then(|token| match token.kind {
            super::TokenKind::Instance(id) => Some(id),
            super::TokenKind::ValueInstance(id) if self.lexer.is_draft() => Some(id),
            _ => None,
        });
        let diagnostic_start = self.diagnostics.len();
        let (id, record) = match self.record() {
            Ok(record) => record,
            Err(error @ ParseError::Resource(_)) => return Err(error),
            Err(error @ ParseError::Lex(_)) => return Err(error),
            Err(error @ ParseError::Syntax { .. }) => {
                self.diagnostics.truncate(diagnostic_start);
                let end = statement_end(
                    self.lexer.input(),
                    start,
                    self.lexer.is_draft(),
                    self.budget,
                )?;
                self.omit_record(
                    start..end,
                    omitted,
                    format_args!("bounded DATA record omitted: {error}; exact source retained"),
                )?;
                if let Some(id) = source_id {
                    if let Some(previous) = records.remove(&id) {
                        self.omit_record(previous.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"))?;
                    }
                    // A readable name in a defective statement cannot give a
                    // later statement permission to select another definition.
                    self.budget
                        .insert_btree_set(ambiguous, id, "step_ambiguous_instance_ids")?;
                }
                self.lexer
                    .set_literal_admission(super::LiteralAdmission::Required);
                self.lexer.seek(end);
                self.last_end = end;
                self.current = self.lex_next()?;
                return Ok(());
            }
        };
        if ambiguous.contains(&id) || records.contains_key(&id) {
            if let Some(previous) = records.remove(&id) {
                self.omit_record(previous.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"))?;
            }
            self.budget
                .insert_btree_set(ambiguous, id, "step_ambiguous_instance_ids")?;
            self.omit_record(record.span, omitted, format_args!("duplicate instance name #{id}; ambiguous instance omitted; exact source retained"))?;
            return Ok(());
        }
        self.budget
            .insert_btree_map(records, id, record, "step_parse_record_table_storage")?;
        Ok(())
    }

    pub(super) fn omit_unresolved_records(
        &mut self,
        records: &mut BTreeMap<u64, RawRecord>,
        external: &BTreeSet<u64>,
        external_values: &BTreeSet<u64>,
        omitted: &mut Vec<Range<usize>>,
    ) -> Result<(), ParseError> {
        let mut pending = Vec::new();
        for (&id, record) in records.iter() {
            let mut missing = None;
            for partial in &record.partials {
                for value in &partial.parameters {
                    visit_references(value, self.budget, &mut |target, value_instance| {
                        if (value_instance && !external_values.contains(&target))
                            || (!value_instance
                                && !records.contains_key(&target)
                                && !external.contains(&target))
                        {
                            missing = Some((target, value_instance));
                        }
                        Ok(())
                    })?;
                }
            }
            if let Some((target, value_instance)) = missing {
                let prefix = if value_instance { '@' } else { '#' };
                self.diagnostic(record.span.start, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{id} omitted: unresolved instance reference {prefix}{target}; exact source retained"))?;
                self.budget
                    .push_vec(&mut pending, id, "step_unresolved_instances")?;
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        // Build the reverse graph only when an omission must propagate. Each
        // edge is indexed once; each removed node unlocks its direct users.
        let mut reverse = BTreeMap::<u64, Vec<u64>>::new();
        let mut storage = self
            .budget
            .reserve_scoped(0, "step_recovery_reverse_storage")?;
        storage.with_storage(|| {
            for (&id, record) in records.iter() {
                for partial in &record.partials {
                    for value in &partial.parameters {
                        visit_references(value, self.budget, &mut |target, value_instance| {
                            if !value_instance {
                                self.budget.push_btree_group(
                                    &mut reverse,
                                    target,
                                    id,
                                    "step_recovery_reverse_nodes",
                                    "step_recovery_reverse_edges",
                                )?;
                            }
                            Ok(())
                        })?;
                    }
                }
            }
            Ok::<(), ParseError>(())
        })?;
        let mut queued = self
            .budget
            .collect_btree_set(pending.iter().copied(), "step_recovery_queued_instances")?;
        while let Some(id) = pending.pop() {
            if let Some(record) = records.remove(&id) {
                self.budget
                    .push_vec(omitted, record.span, "step_omitted_record_spans")?;
            }
            if let Some(users) = reverse.get(&id) {
                for &user in users {
                    self.budget
                        .charge_work(1, "step_recovery_dependency_edge")?;
                    if self.budget.insert_btree_set(
                        &mut queued,
                        user,
                        "step_recovery_queued_instances",
                    )? {
                        if let Some(record) = records.get(&user) {
                            self.diagnostic(record.span.start, ParseDiagnosticKind::RecordOmitted, format_args!("instance #{user} omitted: depends on omitted instance #{id}; exact source retained"))?;
                            self.budget.push_vec(
                                &mut pending,
                                user,
                                "step_unresolved_instances",
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn visit_references(
    value: &Value,
    budget: &DecodeContext<'_>,
    visit: &mut impl FnMut(u64, bool) -> Result<(), ParseError>,
) -> Result<(), ParseError> {
    let _depth = budget.enter_nested("step_reference_visit_depth")?;
    budget.charge_work(1, "step_reference_visit")?;
    match value {
        Value::Reference(id) => visit(*id, false)?,
        Value::ExternalReference(id) => visit(*id, true)?,
        Value::List(values) => {
            for value in values {
                visit_references(value, budget, visit)?;
            }
        }
        Value::Typed(_, value) => visit_references(value, budget, visit)?,
        _ => {}
    }
    Ok(())
}

/// A semicolon outside a literal or comment bounds a defective statement.
/// An unterminated literal or comment supplies no continuation boundary.
fn statement_end(
    input: &[u8],
    start: usize,
    draft: bool,
    budget: &DecodeContext<'_>,
) -> Result<usize, ParseError> {
    let mut at = start;
    while let Some(&byte) = input.get(at) {
        budget.charge_work(1, "step_record_recovery_scan")?;
        if at > start && matches!(byte, b'#' | b'@') {
            let mut next = at + 1;
            let mut digits = false;
            while let Some(&byte) = input.get(next) {
                if !byte.is_ascii_digit() && !byte.is_ascii_control() {
                    break;
                }
                digits |= byte.is_ascii_digit();
                budget.charge_work(1, "step_record_recovery_scan")?;
                next += 1;
            }
            while input
                .get(next)
                .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                budget.charge_work(1, "step_record_recovery_scan")?;
                next += 1;
            }
            if digits && input.get(next) == Some(&b'=') {
                return Ok(at);
            }
        }
        if at > start
            && byte.eq_ignore_ascii_case(&b'E')
            && input[..at]
                .iter()
                .rev()
                .find(|byte| !byte.is_ascii_control())
                .is_some_and(|byte| {
                    !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'!' | b'-')
                })
        {
            if let Some(mut end) = crate::lex::match_ignoring_controls(input, at, b"ENDSEC") {
                while input.get(end).is_some_and(u8::is_ascii_control) {
                    end += 1;
                }
                budget.charge_work(super::u64_from_index(end - at), "step_record_recovery_scan")?;
                if input
                    .get(end)
                    .is_some_and(|byte| matches!(byte, b' ' | b';'))
                {
                    return Ok(at);
                }
            }
        }
        if byte == b';' {
            return Ok(at + 1);
        }
        let close = match input.get(at..at + 2) {
            Some(b"/*") => Some(b"*/"),
            Some(b"!*") if draft => Some(b"*!"),
            _ => None,
        };
        if let Some(close) = close {
            at += 2;
            while input.get(at..at + 2) != Some(close) {
                if at >= input.len() {
                    return Parser::err_at(
                        start,
                        "statement recovery cannot establish a boundary: unterminated comment",
                    );
                }
                budget.charge_work(1, "step_record_recovery_scan")?;
                at += 1;
            }
            at += 2;
            continue;
        }
        let delimiter = match byte {
            b'\'' | b'"' => Some(byte),
            b'<' => Some(b'>'),
            _ => None,
        };
        if let Some(delimiter) = delimiter {
            at += 1;
            loop {
                let Some(&byte) = input.get(at) else {
                    return Parser::err_at(
                        start,
                        "statement recovery cannot establish a boundary: unterminated literal",
                    );
                };
                budget.charge_work(1, "step_record_recovery_scan")?;
                if byte == delimiter {
                    if delimiter == b'\'' {
                        if let Some(end) =
                            crate::lex::match_exact_ignoring_controls(input, at, b"''")
                        {
                            budget.charge_work(
                                super::u64_from_index(end - at),
                                "step_record_recovery_scan",
                            )?;
                            at = end;
                            continue;
                        }
                    }
                    at += 1;
                    break;
                }
                at += 1;
            }
        } else {
            at += 1;
        }
    }
    // EOF is a known boundary for an incomplete record without an open literal.
    Ok(at)
}

#[cfg(test)]
mod tests;
