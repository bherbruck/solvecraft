// SPDX-License-Identifier: Apache-2.0
//! Byte-oriented ISO 10303-21 lexical analysis.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

/// The entity or value occurrence class.
#[derive(Debug, Clone, Copy)]
enum OccurrencePrefix {
    Entity,
    Value,
}

/// A lexical token with its exact source-byte extent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    /// Parsed token category.
    pub(crate) kind: TokenKind,
    /// Half-open byte range in the exchange structure.
    pub(crate) span: Range<usize>,
}

/// Part 21 token categories.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    /// A lexical defect deferred to its containing statement's recovery.
    InvalidSource(String),
    /// Standard keyword or entity name.
    Name(String),
    /// User-defined `!`-prefixed keyword.
    UserName(String),
    /// Numeric `#`-prefixed entity-instance name.
    Instance(u64),
    /// Numeric `@`-prefixed value-instance name.
    ValueInstance(u64),
    /// `#`-prefixed EXPRESS entity constant name.
    ConstantEntity(String),
    /// `@`-prefixed EXPRESS value constant name.
    ConstantValue(String),
    /// Signed decimal integer.
    Integer(i64),
    /// Decimal real, including an optional exponent.
    Real(FiniteReal),
    /// A bounded metadata literal that cannot enter its typed domain.
    UninterpretedLiteral,
    /// Dot-delimited enumeration or logical literal.
    Enumeration(String),
    /// Bytes between apostrophe delimiters, before escape decoding.
    String(Vec<u8>),
    /// Decoded quoted hexadecimal binary literal.
    Binary(BinaryValue),
    /// Edition-3 resource token.
    Resource(String),
    /// Opening parenthesis.
    LParen,
    /// Closing parenthesis.
    RParen,
    /// Parameter separator.
    Comma,
    /// Statement terminator.
    Semicolon,
    /// Assignment operator.
    Equals,
    /// Omitted-value marker `$`.
    Omitted,
    /// Derived-value marker `*`.
    Derived,
    /// Anchor-tag name, preserving source case.
    TagName(String),
    /// Opening anchor-tag delimiter.
    LBrace,
    /// Closing anchor-tag delimiter.
    RBrace,
    /// Anchor-tag name/value separator.
    Colon,
}

/// Binary literal payload packed most-significant nibble first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinaryValue {
    /// Unused low-order bits in the final byte, including nibble padding.
    unused_bits: u8,
    data: Box<[u8]>,
}

impl BinaryValue {
    pub(crate) fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(self.data.len()), operation)?;
        let data = ctx.copy_slice(&self.data, operation)?;
        Ok(Self {
            unused_bits: self.unused_bits,
            data: data.into_boxed_slice(),
        })
    }

    pub(crate) fn bit_len(&self) -> usize {
        self.data.len() * 8 - usize::from(self.unused_bits)
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Lexical failure with a stable byte position.
#[derive(Debug, thiserror::Error)]
#[error("{message} at byte {offset}")]
pub(crate) struct LexError {
    /// Byte offset at which tokenization failed.
    offset: usize,
    /// Violated lexical invariant.
    pub(crate) message: String,
    resource: Option<CodecError>,
}

impl LexError {
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }
    pub(crate) fn into_codec_error(self) -> CodecError {
        match self.resource {
            Some(error) => error,
            None => CodecError::malformed(format_args!("{} at byte {}", self.message, self.offset)),
        }
    }

    pub(crate) fn into_resource_error(self) -> Option<CodecError> {
        self.resource
    }
}

/// Tokenize one complete clear-text exchange structure.
#[cfg(test)]
pub(crate) fn lex_with_context(
    input: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(input, ctx);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

#[derive(Clone, Copy)]
enum LiteralStorage {
    Retained,
    Transient,
}

#[derive(Clone, Copy)]
pub(crate) enum LiteralAdmission {
    Required,
    Metadata,
}

#[derive(Clone, Copy)]
enum ExchangeSyntax {
    Standard,
    Draft,
}

pub(crate) struct Lexer<'a, 'ctx, 'arena> {
    input: &'a [u8],
    budget: &'ctx DecodeContext<'arena>,
    literal_storage: LiteralStorage,
    literal_admission: LiteralAdmission,
    at: usize,
    allow_print_controls: bool,
    previous_was_signature: bool,
    tag_name_expected: bool,
    syntax: ExchangeSyntax,
}

pub(crate) fn print_control_end(input: &[u8], at: usize) -> Option<usize> {
    match_exact_ignoring_controls(input, at, b"\\N\\")
        .or_else(|| match_exact_ignoring_controls(input, at, b"\\F\\"))
}

impl<'a, 'ctx, 'arena> Lexer<'a, 'ctx, 'arena> {
    pub(crate) fn new(input: &'a [u8], ctx: &'ctx DecodeContext<'arena>) -> Self {
        let draft_offset = crate::codec::draft_exchange_offset(input);
        Self {
            input,
            budget: ctx,
            literal_storage: LiteralStorage::Retained,
            literal_admission: LiteralAdmission::Required,
            at: draft_offset.unwrap_or(0),
            allow_print_controls: true,
            previous_was_signature: false,
            tag_name_expected: false,
            syntax: if draft_offset.is_some() {
                ExchangeSyntax::Draft
            } else {
                ExchangeSyntax::Standard
            },
        }
    }

    pub(crate) fn set_transient_literals(&mut self) {
        self.literal_storage = LiteralStorage::Transient;
    }

    pub(crate) fn set_literal_admission(&mut self, admission: LiteralAdmission) {
        self.literal_admission = admission;
    }

    pub(crate) fn allows_uninterpreted_literals(&self) -> bool {
        matches!(self.literal_admission, LiteralAdmission::Metadata)
    }

    pub(crate) fn set_allow_print_controls(&mut self, allow: bool) {
        self.allow_print_controls = allow;
    }

    pub(crate) fn input(&self) -> &[u8] {
        self.input
    }

    pub(crate) fn is_draft(&self) -> bool {
        matches!(self.syntax, ExchangeSyntax::Draft)
    }

    pub(crate) fn seek(&mut self, at: usize) {
        self.at = at;
        self.tag_name_expected = false;
        self.previous_was_signature = false;
    }

    pub(crate) fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        if !self.skip_trivia()? {
            return Ok(None);
        }
        let token = self.token()?;
        let skip_signature =
            self.previous_was_signature && matches!(&token.kind, TokenKind::Semicolon);
        self.previous_was_signature =
            matches!(&token.kind, TokenKind::Name(name) if name == "SIGNATURE");
        if skip_signature {
            self.skip_signature_payload()?;
        }
        Ok(Some(token))
    }

    fn skip_signature_payload(&mut self) -> Result<(), LexError> {
        let start = self.at;
        let name_len = b"ENDSEC".len();
        let mut at = start;
        let mut boundary_allowed = true;
        while at + name_len <= self.input.len() {
            if self.input.get(at..at + 2) == Some(b"/*") {
                let comment_start = at;
                at += 2;
                let Some(end) = self.input[at..].windows(2).position(|w| w == b"*/") else {
                    return Err(Self::error(comment_start, "unterminated comment"));
                };
                at += end + 2;
                boundary_allowed = true;
                continue;
            }
            if let Some(end) = self.print_control_end(at) {
                at = end;
                boundary_allowed = true;
                continue;
            }
            if self
                .input
                .get(at)
                .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                at += 1;
                boundary_allowed = true;
                continue;
            }
            let candidate = at;
            let Some(mut after_name) = match_ignoring_controls(self.input, candidate, b"ENDSEC")
            else {
                at += 1;
                boundary_allowed = false;
                continue;
            };
            while self
                .input
                .get(after_name)
                .is_some_and(|byte| byte.is_ascii_control() || *byte == b' ')
            {
                after_name += 1;
            }
            if boundary_allowed && self.input.get(after_name) == Some(&b';') {
                self.validate_signature_payload(start, candidate)?;
                self.at = candidate;
                return Ok(());
            }
            at += 1;
            boundary_allowed = false;
        }
        Err(Self::error(start, "unterminated signature section"))
    }

    fn validate_signature_payload(&self, start: usize, end: usize) -> Result<(), LexError> {
        let mut quantum_len = 0;
        let mut padding = 0;
        let mut finished = false;
        let mut saw_content = false;
        let mut trailing_separator = false;
        let mut relative = 0;
        while relative < end - start {
            let byte = self.input[start + relative];
            if byte.is_ascii_control() {
                relative += 1;
                continue;
            }
            if byte == b' ' {
                trailing_separator |= saw_content;
                relative += 1;
                continue;
            }
            if let Some(separator_end) = self.print_control_end(start + relative) {
                if separator_end > end {
                    return Err(Self::error(
                        start + relative,
                        "invalid SIGNATURE base64 character",
                    ));
                }
                trailing_separator |= saw_content;
                relative = separator_end - start;
                continue;
            }
            if self.input.get(start + relative..start + relative + 2) == Some(b"/*") {
                let comment_start = start + relative;
                let comment_body = comment_start + 2;
                let Some(comment_end) = self.input[comment_body..end]
                    .windows(2)
                    .position(|window| window == b"*/")
                else {
                    return Err(Self::error(comment_start, "unterminated comment"));
                };
                trailing_separator |= saw_content;
                relative = comment_body + comment_end + 2 - start;
                continue;
            }
            if trailing_separator {
                return Err(Self::error(
                    start + relative,
                    "invalid SIGNATURE base64 character",
                ));
            }
            saw_content = true;
            let is_alphabet = byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
            if finished || (padding != 0 && is_alphabet) {
                return Err(Self::error(
                    start + relative,
                    "invalid SIGNATURE base64 padding",
                ));
            }
            if is_alphabet {
                quantum_len += 1;
            } else if byte == b'=' {
                if quantum_len < 2 || padding == 2 {
                    return Err(Self::error(
                        start + relative,
                        "invalid SIGNATURE base64 padding",
                    ));
                }
                padding += 1;
                quantum_len += 1;
            } else {
                return Err(Self::error(
                    start + relative,
                    "invalid SIGNATURE base64 character",
                ));
            }
            if quantum_len == 4 {
                finished = padding != 0;
                quantum_len = 0;
                padding = 0;
            }
            relative += 1;
        }
        if !saw_content {
            return Err(Self::error(
                start,
                "SIGNATURE section has empty base64 content",
            ));
        }
        if quantum_len != 0 {
            return Err(Self::error(
                end,
                "SIGNATURE base64 content has incomplete quantum",
            ));
        }
        Ok(())
    }

    fn skip_trivia(&mut self) -> Result<bool, LexError> {
        loop {
            while self.input.get(self.at).is_some_and(u8::is_ascii_control) {
                self.at += 1;
            }
            if let Some(end) = self.print_control_end(self.at) {
                if !self.allow_print_controls {
                    return Err(Self::error(
                        self.at,
                        "print control directive is not allowed in this section",
                    ));
                }
                self.at = end;
                continue;
            }
            if self.input.get(self.at) == Some(&b' ') {
                self.at += 1;
                continue;
            }
            let close = match self.input.get(self.at..self.at + 2) {
                Some(b"/*") => b"*/",
                Some(b"!*") if self.is_draft() => b"*!",
                _ => return Ok(self.at < self.input.len()),
            };
            let start = self.at;
            self.at += 2;
            let Some(end) = self.input[self.at..].windows(2).position(|w| w == close) else {
                return Err(Self::error(start, "unterminated comment"));
            };
            self.at += end + 2;
        }
    }

    fn token(&mut self) -> Result<Token, LexError> {
        let start = self.at;
        if self.tag_name_expected {
            self.tag_name_expected = false;
            let kind = self.tag_name()?;
            return Ok(Token {
                kind,
                span: start..self.at,
            });
        }
        let byte = self.input[self.at];
        let kind = match byte {
            b'(' => self.one(TokenKind::LParen),
            b')' => self.one(TokenKind::RParen),
            b',' => self.one(TokenKind::Comma),
            b';' => self.one(TokenKind::Semicolon),
            b'=' => self.one(TokenKind::Equals),
            b'{' => self.one(TokenKind::LBrace),
            b'}' => self.one(TokenKind::RBrace),
            b':' => self.one(TokenKind::Colon),
            b'$' => self.one(TokenKind::Omitted),
            b'*' => self.one(TokenKind::Derived),
            b'#' => self.occurrence(OccurrencePrefix::Entity)?,
            b'@' => self.occurrence(OccurrencePrefix::Value)?,
            b'\'' => self.string()?,
            b'"' => self.binary()?,
            b'<' => self.resource()?,
            b'.' if self
                .next_non_ignored(self.at + 1)
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_') =>
            {
                self.enumeration()?
            }
            b'!' => self.user_name()?,
            b'+' | b'-' | b'0'..=b'9' | b'.' => self.number()?,
            b if b.is_ascii_alphabetic() || b == b'_' => TokenKind::Name(self.name()?),
            _ => return Err(Self::error(start, "unexpected byte")),
        };
        Ok(Token {
            kind,
            span: start..self.at,
        })
    }

    fn one(&mut self, kind: TokenKind) -> TokenKind {
        self.at += 1;
        if matches!(kind, TokenKind::LBrace) {
            self.tag_name_expected = true;
        }
        kind
    }

    fn name(&mut self) -> Result<String, LexError> {
        let start = self.at;
        self.at += 1;
        while self.input.get(self.at).is_some_and(|b| {
            b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-' || b.is_ascii_control()
        }) {
            self.at += 1;
        }
        let (mut name, _) = self.normalized(start, self.at, LiteralStorage::Retained)?;
        name.make_ascii_uppercase();
        Ok(name)
    }

    fn tag_name(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        if !self
            .input
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(Self::error(start, "tag name has no identifier"));
        }
        self.at += 1;
        while self.input.get(self.at).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || *byte == b'_' || byte.is_ascii_control()
        }) {
            self.at += 1;
        }
        let (name, _) = self.normalized(start, self.at, LiteralStorage::Retained)?;
        Ok(TokenKind::TagName(name))
    }

    fn user_name(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored();
        if !self
            .input
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(Self::error(start, "user-defined name has no identifier"));
        }
        Ok(TokenKind::UserName(self.name()?))
    }

    fn occurrence(&mut self, prefix: OccurrencePrefix) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored();
        match self.input.get(self.at).copied() {
            Some(byte) if byte.is_ascii_digit() => {
                let digits = self.at;
                while let Some(byte) = self.input.get(self.at).copied() {
                    if byte.is_ascii_digit() || byte.is_ascii_control() {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let (raw, _temporary) =
                    self.normalized(digits, self.at, LiteralStorage::Transient)?;
                let value = raw
                    .parse::<u64>()
                    .ok()
                    .ok_or_else(|| Self::error(start, "instance name is out of range"))?;
                if value == 0 {
                    return Err(Self::error(start, "instance name must not be zero"));
                }
                match prefix {
                    OccurrencePrefix::Entity => Ok(TokenKind::Instance(value)),
                    OccurrencePrefix::Value => Ok(TokenKind::ValueInstance(value)),
                }
            }
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                let name_start = self.at;
                self.at += 1;
                while let Some(byte) = self.input.get(self.at).copied() {
                    if byte.is_ascii_alphanumeric() || byte == b'_' || byte.is_ascii_control() {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                let (mut name, _) =
                    self.normalized(name_start, self.at, LiteralStorage::Retained)?;
                name.make_ascii_uppercase();
                match prefix {
                    OccurrencePrefix::Entity => Ok(TokenKind::ConstantEntity(name)),
                    OccurrencePrefix::Value => Ok(TokenKind::ConstantValue(name)),
                }
            }
            _ => Err(Self::error(start, "occurrence name has no identifier")),
        }
    }

    fn number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        if matches!(self.input[self.at], b'+' | b'-') {
            self.at += 1;
            self.skip_ignored();
        }
        let mut dot = false;
        let mut exponent = false;
        while let Some(&b) = self.input.get(self.at) {
            match b {
                byte if byte.is_ascii_control() => self.at += 1,
                b'0'..=b'9' => self.at += 1,
                b'.' if !dot => {
                    dot = true;
                    self.at += 1;
                }
                b'E' | b'e' | b'D' | b'd' if !exponent => {
                    exponent = true;
                    self.at += 1;
                    self.skip_ignored();
                    if self
                        .input
                        .get(self.at)
                        .is_some_and(|b| matches!(b, b'+' | b'-'))
                    {
                        self.at += 1;
                        self.skip_ignored();
                    }
                }
                _ => break,
            }
        }
        let (mut raw, _temporary) = self.normalized(start, self.at, LiteralStorage::Transient)?;
        if exponent && raw.ends_with('.') {
            raw.pop();
        }
        let admitted = if dot || exponent {
            raw.make_ascii_uppercase();
            let mut index = 0;
            while index < raw.len() {
                if raw.as_bytes()[index] == b'D' {
                    raw.replace_range(index..=index, "E");
                }
                index += 1;
            }
            raw.parse::<f64>()
                .map_err(|_| Self::error(start, "invalid real"))
                .and_then(|value| {
                    FiniteReal::new(value)
                        .map(TokenKind::Real)
                        .ok_or_else(|| Self::error(start, "real exceeds finite binary64 range"))
                })
        } else {
            raw.parse()
                .map(TokenKind::Integer)
                .map_err(|_| Self::error(start, "invalid integer"))
        };
        match (self.literal_admission, admitted) {
            (LiteralAdmission::Metadata, Err(_)) => Ok(TokenKind::UninterpretedLiteral),
            (_, result) => result,
        }
    }

    fn enumeration(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        self.skip_ignored();
        let name_start = self.at;
        while self
            .input
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || b.is_ascii_control())
        {
            self.at += 1;
        }
        self.skip_ignored();
        if self.input.get(self.at) != Some(&b'.') {
            return Err(Self::error(start, "unterminated enumeration"));
        }
        let (mut name, _) = self.normalized(name_start, self.at, LiteralStorage::Retained)?;
        name.make_ascii_uppercase();
        self.at += 1;
        Ok(TokenKind::Enumeration(name))
    }

    fn string(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        loop {
            self.budget
                .charge_work(1, "step_string_boundary_scan")
                .map_err(|error| Self::resource_error(start, error))?;
            match self.input.get(self.at).copied() {
                Some(b'\'') => {
                    if let Some(end) = self.match_exact_ignoring_controls(self.at, b"''") {
                        self.at = end;
                    } else {
                        let mut bytes = self
                            .budget
                            .copy_retained(&self.input[content..self.at], "step_string_lexeme")
                            .map_err(|error| Self::resource_error(start, error))?;
                        self.budget
                            .charge_work(u64_from_index(bytes.len()), "step_string_normalization")
                            .map_err(|error| Self::resource_error(start, error))?;
                        bytes.retain(|byte| !byte.is_ascii_control());
                        self.at += 1;
                        return Ok(TokenKind::String(bytes));
                    }
                }
                Some(byte) if byte.is_ascii_control() => {
                    self.at += 1;
                }
                Some(b'\\') => {
                    if let Some(end) = self.print_control_end(self.at) {
                        if !self.allow_print_controls {
                            return Err(Self::error(
                                self.at,
                                "print control directive is not allowed in this section",
                            ));
                        }
                        self.at = end;
                    } else {
                        self.at += 1;
                    }
                }
                Some(_) => self.at += 1,
                None => return Err(Self::error(start, "unterminated string")),
            }
        }
    }

    fn binary(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.binary_value()
            .or_else(|error| self.recover_metadata_literal(start, b'"', error))
    }

    fn recover_metadata_literal(
        &mut self,
        start: usize,
        closing: u8,
        error: LexError,
    ) -> Result<TokenKind, LexError> {
        if error.resource.is_some() || !matches!(self.literal_admission, LiteralAdmission::Metadata)
        {
            return Err(error);
        }
        for end in start + 1..self.input.len() {
            self.budget
                .charge_work(1, "STEP metadata literal recovery")
                .map_err(|error| Self::resource_error(start, error))?;
            if self.input[end] == closing {
                self.at = end + 1;
                return Ok(TokenKind::UninterpretedLiteral);
            }
        }
        Err(error)
    }

    fn binary_value(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        let mut digit_count = 0usize;
        while let Some(byte) = self.input.get(self.at).copied() {
            if HexDigit::new(byte).is_some() {
                digit_count += 1;
                self.at += 1;
            } else if byte.is_ascii_control() {
                self.at += 1;
            } else if byte == b'\\' {
                let Some(after_print_control) = self.print_control_end(self.at) else {
                    break;
                };
                if !self.allow_print_controls {
                    return Err(Self::error(
                        self.at,
                        "print control directive is not allowed in this section",
                    ));
                }
                self.at = after_print_control;
            } else {
                break;
            }
        }
        if self.input.get(self.at) != Some(&b'"') {
            return Err(Self::error(start, "invalid binary literal"));
        }
        let _temporary = self
            .budget
            .reserve_scoped(u64_from_index(digit_count), "step_binary_lexeme_temp")
            .map_err(|error| Self::resource_error(start, error))?;
        let mut raw = self
            .budget
            .alloc_filled_admitted(digit_count, HexDigit(0), "step_binary_hex_digits")
            .map_err(|error| Self::resource_error(start, error))?;
        let mut cursor = content;
        let mut written = 0usize;
        while cursor < self.at {
            let byte = self.input[cursor];
            if let Some(digit) = HexDigit::new(byte) {
                raw[written] = digit;
                written += 1;
                cursor += 1;
            } else if byte.is_ascii_control() {
                cursor += 1;
            } else if byte == b'\\' {
                let Some(end) = self.print_control_end(cursor) else {
                    return Err(Self::error(cursor, "invalid binary literal"));
                };
                cursor = end;
            } else {
                return Err(Self::error(cursor, "invalid binary literal"));
            }
        }
        let Some((&indicator, digits)) = raw.split_first() else {
            return Err(Self::error(
                start,
                "binary literal has no unused-bit indicator",
            ));
        };
        let unused_bits = indicator.nibble();
        if unused_bits > 3 {
            return Err(Self::error(
                start,
                "binary unused-bit indicator exceeds three",
            ));
        }
        if digits.is_empty() && unused_bits != 0 {
            return Err(Self::error(start, "empty binary payload has unused bits"));
        }
        if unused_bits != 0
            && digits
                .last()
                .is_some_and(|digit| digit.nibble() & ((1 << unused_bits) - 1) != 0)
        {
            return Err(Self::error(start, "unused binary bits are not zero"));
        }
        let packed_len = digits.len().div_ceil(2);
        let _packed_temporary = if matches!(self.literal_storage, LiteralStorage::Transient) {
            Some(
                self.budget
                    .reserve_scoped(u64_from_index(packed_len), "step_binary_packed_temp")
                    .map_err(|error| Self::resource_error(start, error))?,
            )
        } else {
            self.budget
                .charge_retained(u64_from_index(packed_len), "step_binary_lexeme_retained")
                .map_err(|error| Self::resource_error(start, error))?;
            None
        };
        let mut data = self
            .budget
            .alloc_filled_admitted(packed_len, 0_u8, "step_binary_packed_bytes")
            .map_err(|error| Self::resource_error(start, error))?;
        let mut output = 0usize;
        let mut pairs = digits.chunks_exact(2);
        for pair in &mut pairs {
            data[output] = (pair[0].nibble() << 4) | pair[1].nibble();
            output += 1;
        }
        if let [last] = pairs.remainder() {
            data[output] = last.nibble() << 4;
        }
        let unused_bits = unused_bits + if digits.len() % 2 == 1 { 4 } else { 0 };
        self.at += 1;
        Ok(TokenKind::Binary(BinaryValue {
            unused_bits,
            data: data.into_boxed_slice(),
        }))
    }

    fn resource(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.resource_value()
            .or_else(|error| self.recover_metadata_literal(start, b'>', error))
    }

    fn resource_value(&mut self) -> Result<TokenKind, LexError> {
        let start = self.at;
        self.at += 1;
        let content = self.at;
        let mut value_len = 0usize;
        while let Some(byte) = self.input.get(self.at).copied() {
            if byte == b'>' {
                break;
            }
            if self.print_control_end(self.at).is_some() {
                return Err(Self::error(
                    self.at,
                    "print control directive is not allowed in a resource",
                ));
            }
            if !byte.is_ascii_control() {
                value_len += 1;
            }
            self.at += 1;
        }
        if self.input.get(self.at) != Some(&b'>') {
            return Err(Self::error(start, "unterminated resource token"));
        }
        let _temporary = self
            .budget
            .reserve_scoped(u64_from_index(value_len), "step_uri_lexeme_temp")
            .map_err(|error| Self::resource_error(start, error))?;
        if matches!(self.literal_storage, LiteralStorage::Retained) {
            self.budget
                .charge_retained(u64_from_index(value_len), "step_uri_lexeme_retained")
                .map_err(|error| Self::resource_error(start, error))?;
        }
        let mut value = self
            .budget
            .alloc_filled_admitted(value_len, 0_u8, "step_uri_lexeme_bytes")
            .map_err(|error| Self::resource_error(start, error))?;
        let mut written = 0usize;
        for &byte in &self.input[content..self.at] {
            if !byte.is_ascii_control() {
                value[written] = byte;
                written += 1;
            }
        }
        let value = String::from_utf8(value)
            .map_err(|_| Self::error(content, "resource token is not UTF-8"))?;
        self.at += 1;
        Ok(TokenKind::Resource(value))
    }

    fn skip_ignored(&mut self) {
        while self.input.get(self.at).is_some_and(u8::is_ascii_control) {
            self.at += 1;
        }
    }

    fn next_non_ignored(&self, mut at: usize) -> Option<u8> {
        while self.input.get(at).is_some_and(u8::is_ascii_control) {
            at += 1;
        }
        self.input.get(at).copied()
    }

    fn normalized(
        &self,
        start: usize,
        end: usize,
        storage: LiteralStorage,
    ) -> Result<(String, Option<ScopedReservation<'_>>), LexError> {
        let byte_count = self.input[start..end]
            .iter()
            .filter(|byte| !byte.is_ascii_control())
            .map(|byte| char::from(*byte).len_utf8())
            .sum::<usize>();
        let operation = match storage {
            LiteralStorage::Retained => "step_lex_normalized_retained",
            LiteralStorage::Transient => "step_lex_normalized_temp",
        };
        let (mut output, reservation) = match storage {
            LiteralStorage::Retained => (
                self.budget
                    .retained_string(byte_count, operation)
                    .map_err(|error| Self::resource_error(start, error))?,
                None,
            ),
            LiteralStorage::Transient => {
                let mut reservation = self
                    .budget
                    .reserve_scoped(0, operation)
                    .map_err(|error| Self::resource_error(start, error))?;
                let mut output = String::new();
                self.budget
                    .reserve_scoped_string(&mut reservation, &mut output, byte_count, operation)
                    .map_err(|error| Self::resource_error(start, error))?;
                (output, Some(reservation))
            }
        };
        for &byte in &self.input[start..end] {
            if !byte.is_ascii_control() {
                output.push(char::from(byte));
            }
        }
        Ok((output, reservation))
    }

    fn print_control_end(&self, at: usize) -> Option<usize> {
        print_control_end(self.input, at)
    }

    fn match_exact_ignoring_controls(&self, at: usize, expected: &[u8]) -> Option<usize> {
        match_exact_ignoring_controls(self.input, at, expected)
    }

    fn error(offset: usize, message: &str) -> LexError {
        LexError {
            offset,
            message: message.into(),
            resource: None,
        }
    }

    fn resource_error(offset: usize, error: CodecError) -> LexError {
        LexError {
            offset,
            message: error.to_string(),
            resource: Some(error),
        }
    }
}

/// An ASCII hexadecimal digit, carrying the four bits it names.
///
/// The type makes the nibble map total: a value of this type exists only
/// because the byte it came from names a hexadecimal digit, so the map from a
/// scanned binary literal to its nibbles has no unreachable arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HexDigit(u8);

impl HexDigit {
    /// The digit an ASCII byte names, or `None` when the byte names none.
    const fn new(byte: u8) -> Option<Self> {
        Some(Self(match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        }))
    }

    /// The four bits the digit names, 0 through 15.
    const fn nibble(self) -> u8 {
        self.0
    }
}

pub(crate) fn match_exact_ignoring_controls(
    input: &[u8],
    mut at: usize,
    expected: &[u8],
) -> Option<usize> {
    for &byte in expected {
        while input.get(at).is_some_and(u8::is_ascii_control) {
            at += 1;
        }
        if input.get(at) != Some(&byte) {
            return None;
        }
        at += 1;
    }
    Some(at)
}

pub(crate) fn match_ignoring_controls(
    input: &[u8],
    mut at: usize,
    expected: &[u8],
) -> Option<usize> {
    for &byte in expected {
        while input.get(at).is_some_and(u8::is_ascii_control) {
            at += 1;
        }
        if !input
            .get(at)
            .is_some_and(|value| value.eq_ignore_ascii_case(&byte))
        {
            return None;
        }
        at += 1;
    }
    Some(at)
}

#[cfg(test)]
mod tests;
