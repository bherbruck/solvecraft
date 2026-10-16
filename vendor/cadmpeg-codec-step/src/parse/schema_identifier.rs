// SPDX-License-Identifier: Apache-2.0
//! Part 21 schema identifier classification.
//!
//! A schema identifier is a schema name and an optional ISO/IEC 8824-1 object
//! identifier. The classifier is total: an identifier is valid, recoverable
//! under its schema name alone, or invalid. `valid_schema_identifier` is the
//! strict predicate; only the `FILE_SCHEMA` admission point reads the
//! recoverable form.
//!
//! One rule separates the recoverable form from the invalid form. A component
//! outside the range that its position permits is recoverable: the source
//! states which components it asserts, and the classifier reports the range
//! defect. Text that does not parse into components is invalid: the source
//! states no components to report.
//!
//! The root component states a root number as a number or as a registered
//! ASN.1 identifier, so one range rule covers both spellings.
//!
//! This module owns the one schema-name and object-identifier split.
//! `split_schema_identifier` serves the admission here, the DATA section and
//! `FILE_POPULATION` schema-name match, and the AP242 edition report.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// One `FILE_SCHEMA` identifier that the header admits.
///
/// The header classifies each identifier once and keeps the result. The
/// decoded text is owned: it comes from a string decode that has no home in the
/// header record, so a borrow would have nothing to point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdmittedSchemaIdentifier {
    state: AdmittedSchemaState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdmittedSchemaState {
    /// The schema name and the optional object identifier are both valid.
    Valid {
        /// The decoded identifier text, as the source states it.
        text: String,
    },
    /// The object identifier parses, and one component is outside the range
    /// that its position permits.
    ObjectIdentifierOutOfRange {
        /// The decoded identifier text, as the source states it.
        text: String,
        /// The schema name, without the object identifier.
        name: String,
        /// The first component in source order that is out of range.
        component: String,
    },
}

impl AdmittedSchemaIdentifier {
    /// Admit one decoded identifier, or reject it.
    pub(super) fn admit(identifier: String) -> Option<Self> {
        let out_of_range = match schema_identifier_form(&identifier) {
            SchemaIdentifierForm::Valid => None,
            SchemaIdentifierForm::ObjectIdentifierOutOfRange { name, component } => {
                Some((name.to_owned(), component.to_owned()))
            }
            SchemaIdentifierForm::Invalid => return None,
        };
        Some(Self {
            state: match out_of_range {
                None => AdmittedSchemaState::Valid { text: identifier },
                Some((name, component)) => AdmittedSchemaState::ObjectIdentifierOutOfRange {
                    text: identifier,
                    name,
                    component,
                },
            },
        })
    }

    /// The decoded identifier text, as the source states it.
    pub(super) fn text(&self) -> &str {
        match &self.state {
            AdmittedSchemaState::Valid { text }
            | AdmittedSchemaState::ObjectIdentifierOutOfRange { text, .. } => text,
        }
    }

    /// The proved schema name and first out-of-range component, when present.
    pub(super) fn out_of_range(&self) -> Option<(&str, &str)> {
        match &self.state {
            AdmittedSchemaState::Valid { .. } => None,
            AdmittedSchemaState::ObjectIdentifierOutOfRange {
                name, component, ..
            } => Some((name, component)),
        }
    }

    /// Numeric object-identifier components, with registered root names mapped
    /// to their assigned number.
    pub(super) fn numeric_object_identifier(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<Vec<u64>>, CodecError> {
        let Some((_, Some(object_identifier))) = split_schema_identifier(self.text()) else {
            return Ok(None);
        };
        let mut components = object_identifier.split_whitespace();
        let Some(root) = components.next().and_then(schema_oid_root_number) else {
            return Ok(None);
        };
        let mut numbers = Vec::new();
        ctx.push_vec(
            &mut numbers,
            u64::from(root),
            "step_schema_object_identifier_components",
        )?;
        for component in components {
            let ComponentForm::Number(number) = schema_oid_component_form(component) else {
                return Ok(None);
            };
            let Ok(number) = number.parse() else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut numbers,
                number,
                "step_schema_object_identifier_components",
            )?;
        }
        Ok((numbers.len() >= 2).then_some(numbers))
    }
}

/// The admission form of one schema identifier.
enum SchemaIdentifierForm<'a> {
    /// The schema name and the optional object identifier are both valid.
    Valid,
    /// The object identifier parses, and one component is outside the range
    /// that its position permits.
    ObjectIdentifierOutOfRange {
        /// The schema name, without the object identifier.
        name: &'a str,
        /// The first component in source order that is out of range.
        component: &'a str,
    },
    /// The identifier does not parse.
    Invalid,
}

fn schema_identifier_form(identifier: &str) -> SchemaIdentifierForm<'_> {
    let identifier = identifier.trim();
    if identifier.is_empty() || identifier.chars().count() > 1024 {
        return SchemaIdentifierForm::Invalid;
    }
    let Some((name, object_identifier)) = split_schema_identifier(identifier) else {
        return SchemaIdentifierForm::Invalid;
    };
    if !valid_schema_name(name) {
        return SchemaIdentifierForm::Invalid;
    }
    let Some(object_identifier) = object_identifier else {
        return SchemaIdentifierForm::Valid;
    };
    match schema_object_identifier_form(object_identifier) {
        ObjectIdentifierForm::Valid => SchemaIdentifierForm::Valid,
        ObjectIdentifierForm::ComponentOutOfRange(component) => {
            SchemaIdentifierForm::ObjectIdentifierOutOfRange { name, component }
        }
        ObjectIdentifierForm::Invalid => SchemaIdentifierForm::Invalid,
    }
}

/// Split one schema identifier into its schema name and the text between the
/// object identifier braces.
///
/// Leading and trailing whitespace around the identifier and around the schema
/// name is ignored. An identifier with no brace is a schema name alone. An
/// identifier that opens an object identifier and does not close it at the end
/// of the identifier has no schema name and no object identifier.
pub(crate) fn split_schema_identifier(identifier: &str) -> Option<(&str, Option<&str>)> {
    let identifier = identifier.trim();
    let Some((name, object_identifier)) = identifier.split_once('{') else {
        return Some((identifier, None));
    };
    let object_identifier = object_identifier.strip_suffix('}')?;
    Some((name.trim_end(), Some(object_identifier)))
}

pub(super) fn valid_schema_identifier(identifier: &str) -> bool {
    matches!(
        schema_identifier_form(identifier),
        SchemaIdentifierForm::Valid
    )
}

fn valid_schema_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The position of the root component in an object identifier.
const ROOT_INDEX: usize = 0;
/// The position of the second component in an object identifier.
const SECOND_INDEX: usize = 1;

/// The admission form of one object identifier.
enum ObjectIdentifierForm<'a> {
    /// Every component parses, and every component is in the range that its
    /// position permits.
    Valid,
    /// Every component parses, and this component is out of range.
    ComponentOutOfRange(&'a str),
    /// The object identifier does not parse.
    Invalid,
}

fn schema_object_identifier_form(value: &str) -> ObjectIdentifierForm<'_> {
    let mut components = value.split_whitespace();
    let Some(first) = components.next() else {
        return ObjectIdentifierForm::Invalid;
    };
    let Some(second) = components.next() else {
        return ObjectIdentifierForm::Invalid;
    };
    let root = schema_oid_root_number(first);
    let mut out_of_range = None;
    for (index, component) in [first, second].into_iter().chain(components).enumerate() {
        let form = schema_oid_component_form(component);
        if matches!(form, ComponentForm::Invalid) {
            return ObjectIdentifierForm::Invalid;
        }
        if schema_oid_component_out_of_range(index, root, &form) {
            out_of_range = out_of_range.or(Some(component));
        }
    }
    out_of_range.map_or(ObjectIdentifierForm::Valid, |component| {
        ObjectIdentifierForm::ComponentOutOfRange(component)
    })
}

/// The admission form of one object identifier component.
enum ComponentForm<'a> {
    /// The component is an ASN.1 identifier, or an ASN.1 identifier followed
    /// by parentheses containing an ASN.1 identifier. Its text gives no number.
    Unnumbered,
    /// The component number is a non-negative decimal number.
    Number(&'a str),
    /// The component parses, and its number carries a minus sign.
    Negative,
    /// The component does not parse.
    Invalid,
}

fn schema_oid_component_form(component: &str) -> ComponentForm<'_> {
    if valid_schema_oid_name(component) {
        return ComponentForm::Unnumbered;
    }
    let Some((name, number)) = component.split_once('(') else {
        return schema_oid_number_form(component);
    };
    let Some(number) = number.strip_suffix(')') else {
        return ComponentForm::Invalid;
    };
    if !valid_schema_oid_name(name) {
        return ComponentForm::Invalid;
    }
    if valid_schema_oid_name(number) {
        return ComponentForm::Unnumbered;
    }
    schema_oid_number_form(number)
}

fn schema_oid_number_form(value: &str) -> ComponentForm<'_> {
    if valid_schema_oid_number(value) {
        return ComponentForm::Number(value);
    }
    if value.strip_prefix('-').is_some_and(valid_schema_oid_number) {
        return ComponentForm::Negative;
    }
    ComponentForm::Invalid
}

/// True when one component is outside the range that its position permits.
///
/// The root component is out of range when its text states no root number. Its
/// text states a root number when the text is `0`, `1`, or `2`, or a registered
/// ASN.1 identifier for one of those numbers, so this one rule holds a root
/// number and a root name to the same range.
///
/// After the root, an object identifier component number is non-negative, so a
/// minus sign is out of range. Under root `0` or `1`, the second component
/// number is in `0..=39`. Every other position permits every non-negative
/// number, and a component whose text states no number states no number to
/// place out of range.
fn schema_oid_component_out_of_range(
    index: usize,
    root: Option<u8>,
    form: &ComponentForm<'_>,
) -> bool {
    if index == ROOT_INDEX {
        return root.is_none();
    }
    if matches!(form, ComponentForm::Negative) {
        return true;
    }
    index == SECOND_INDEX
        && matches!(root, Some(0 | 1))
        && matches!(*form, ComponentForm::Number(number) if !valid_schema_oid_second_arc(number))
}

/// True when a second component number is in `0..=39`.
///
/// The number has no leading zero, so one digit is at most `9`, and two digits
/// are in the range when the first digit is at most `3`.
fn valid_schema_oid_second_arc(number: &str) -> bool {
    number.len() < 2 || (number.len() == 2 && number.as_bytes()[0] <= b'3')
}

fn valid_schema_oid_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn valid_schema_oid_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut previous_hyphen = false;
    for byte in bytes {
        if byte.is_ascii_alphabetic() || byte.is_ascii_digit() {
            previous_hyphen = false;
        } else if byte == b'-' && !previous_hyphen {
            previous_hyphen = true;
        } else {
            return false;
        }
    }
    !value.ends_with('-')
}

/// The number of the root component, when the component text gives one.
///
/// A root number is `0`, `1`, or `2`, written as that number or as a registered
/// ASN.1 identifier for that root. The identifier comparison is exact, so an
/// identifier that differs in case or in spelling from a registered identifier
/// gives no root number. Every other component text gives no root number.
fn schema_oid_root_number(component: &str) -> Option<u8> {
    match schema_oid_component_form(component) {
        ComponentForm::Number("0") => Some(0),
        ComponentForm::Number("1") => Some(1),
        ComponentForm::Number("2") => Some(2),
        ComponentForm::Unnumbered => schema_oid_named_root_value(component),
        ComponentForm::Number(_) | ComponentForm::Negative | ComponentForm::Invalid => None,
    }
}

fn schema_oid_named_root_value(component: &str) -> Option<u8> {
    match component {
        "itu-t" | "ccitt" => Some(0),
        "iso" => Some(1),
        "joint-iso-itu-t" | "joint-iso-ccitt" => Some(2),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
