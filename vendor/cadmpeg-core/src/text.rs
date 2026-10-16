// SPDX-License-Identifier: Apache-2.0
//! Source text that carries its own non-blank proof.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::decode::DecodeContext;
use crate::CodecError;

/// A character that is not whitespace.
///
/// The type exists so [`NonBlankString::prefixed`] is total: a prefix of this
/// type makes the built string hold at least one non-whitespace character
/// whatever the suffix renders to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonWhitespaceChar(char);

impl NonWhitespaceChar {
    /// Admit an ASCII byte that is not whitespace.
    #[must_use]
    pub const fn from_ascii(byte: u8) -> Option<Self> {
        if byte.is_ascii() && !matches!(byte, b'\t'..=b'\r' | b' ') {
            // endian-exception: reconstructed-scalar
            match char::from_u32(u32::from_be_bytes([0, 0, 0, byte])) {
                Some(character) => Some(Self(character)),
                None => None,
            }
        } else {
            None
        }
    }

    /// The lowercase hexadecimal digit naming the low four bits of `nibble`.
    ///
    /// Every hexadecimal digit is non-whitespace, and the mask makes the four
    /// bits total over `u8`, so this constructor refuses nothing.
    #[must_use]
    pub fn hex_digit(nibble: u8) -> Self {
        const DIGITS: [char; 16] = [
            '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        ];
        Self(DIGITS[usize::from(nibble & 0x0f)])
    }
}

impl std::fmt::Display for NonWhitespaceChar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

/// A source string that holds at least one non-whitespace character.
///
/// A selection id, an external document identity and a native name are read
/// back and compared as text. A run of spaces names nothing, so it is refused
/// here rather than by each reader.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NonBlankString(String);

#[cfg(feature = "schema")]
impl JsonSchema for NonBlankString {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "NonBlankString".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Unicode White_Space matches char::is_whitespace; ECMAScript \s does not.
        schemars::json_schema!({
            "type": "string",
            "pattern": r"[^\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]"
        })
    }
}

impl NonBlankString {
    /// Constructs a source string that is not blank.
    ///
    /// Absent when the value holds no non-whitespace character, the empty
    /// string included.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        value
            .chars()
            .any(|character| !character.is_whitespace())
            .then_some(Self(value))
    }

    /// Constructs a non-blank string from a leading character and a suffix.
    ///
    /// Total: the prefix is non-whitespace, so the result holds it whatever
    /// the suffix renders to.
    pub fn prefixed(prefix: NonWhitespaceChar, suffix: impl std::fmt::Display) -> Self {
        Self(format!("{prefix}{suffix}"))
    }

    /// Append text while retaining the admitted non-whitespace character.
    #[must_use]
    pub fn with_suffix(&self, suffix: &str) -> Self {
        let mut value = self.0.clone();
        value.push_str(suffix);
        Self(value)
    }

    /// Returns the source string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Copy a previously admitted non-blank value into decoder-retained storage.
    pub fn try_clone_for_decode(
        &self,
        ctx: &crate::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, crate::CodecError> {
        let text = ctx.copy_retained_text(self.as_str(), operation)?;
        Self::new(text).ok_or_else(|| crate::CodecError::malformed("non-blank text copy changed"))
    }

    /// Consumes the value and returns the source string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Builds a [`NonBlankString`] from a format literal whose first character is
/// literal, non-whitespace text.
///
/// The template is always a literal, so the leading byte is the initializer of
/// a `const` *item*: a template that starts with whitespace, with a non-ASCII
/// byte or with a `{` placeholder fails `cargo check` with E0080, not only a
/// codegen build. (An inline `const { … }` block is evaluated at codegen, so
/// `check` would pass a whitespace literal.) Formatted arguments follow that
/// literal prefix and cannot make the result blank.
///
/// ```compile_fail
/// let _ = cadmpeg_core::nonblank_literal!(" {}", "name");
/// ```
///
/// ```compile_fail
/// let _ = cadmpeg_core::nonblank_literal!("\x0b");
/// ```
#[macro_export]
macro_rules! nonblank_literal {
    ($template:literal $(, $argument:expr)* $(,)?) => {{
        const NONBLANK_LITERAL_LEADING: $crate::text::NonWhitespaceChar = {
            let bytes = $template.as_bytes();
            assert!(
                !bytes.is_empty() && bytes[0] != b'{',
                "a nonblank literal must start with literal ASCII text",
            );
            match $crate::text::NonWhitespaceChar::from_ascii(bytes[0]) {
                Some(character) => character,
                None => panic!("a nonblank literal starts with a non-whitespace ASCII character"),
            }
        };
        let rendered = format!($template $(, $argument)*);
        $crate::text::NonBlankString::prefixed(NONBLANK_LITERAL_LEADING, &rendered[1..])
    }};
}

/// Lookup by the plain string the key spells.
///
/// `Ord` and `Hash` are derived over the same `String`, so a map keyed by this
/// type answers `get`, `contains_key` and indexing with a `&str` and orders its
/// keys exactly as a `String` key would.
impl std::borrow::Borrow<str> for NonBlankString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// A source property the reader could not key.
///
/// A blank key names nothing, so the entry it carries cannot be asked for. A
/// restated key names an entry that is already keyed, so the second value has
/// nowhere to go. Either way the value is still source content, so the reader
/// states the property it could not key instead of dropping it without a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedEntryError {
    /// The resource policy or allocator refused the operation.
    ResourceRefusal(crate::decode::ResourceLimit),
    /// The owning record could not be formatted.
    FormattingRefusal,
    /// The key holds no non-whitespace character.
    Blank {
        /// The record the reader named as the owner of the property set.
        record: String,
    },
    /// The record states this key a second time.
    Restated {
        /// The record the reader named as the owner of the property set.
        record: String,
        /// The key the record states twice.
        key: NonBlankString,
    },
}

impl std::fmt::Display for NamedEntryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceRefusal(limit) => write!(
                formatter,
                "resource refusal during {}: {:?}",
                limit.operation, limit.dimension
            ),
            Self::FormattingRefusal => formatter.write_str("cannot format named entry record"),
            Self::Blank { record } => {
                write!(formatter, "{record} states a property with a blank key")
            }
            Self::Restated { record, key } => write!(
                formatter,
                "{record} states the property {key} a second time"
            ),
        }
    }
}

impl std::error::Error for NamedEntryError {}

impl NamedEntryError {
    fn from_refusal(error: &CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::ResourceRefusal(*limit),
            _ => Self::FormattingRefusal,
        }
    }
}

/// Keys a map by the entry names, and states every name it cannot key.
///
/// Returns the entries whose keys name something and are stated once, and one
/// [`NamedEntryError`] per entry whose key is blank or restated, in the order
/// the reader supplied. `record` names the owning record so the refused
/// property can be reported against the record that states it; it is rendered
/// only when a key is refused.
///
/// The first value a key states is the one that is kept, so a later restatement
/// never overwrites what is already keyed. The blank key names its record and
/// nothing else: the entries arrive as an iterator whose order this function
/// does not know, so a position counted here would name nothing in the source.
/// A reader that does know where in its source the property sits puts that in
/// `record`, which it builds. A restated key names itself as well as the
/// record, because the key is what the source states twice.
///
/// This is the route for a reader that keeps the properties it can key and
/// reports the rest. A reader that refuses the whole set uses
/// [`named_entries_for_decode`].
///
/// Keys named entries after the caller admits each new map node and refusal.
///
/// The caller context charges collection slots and retained storage before
/// each vacant map node and refused-entry vector append. Refusal text and
/// restated keys are copied after retained-byte
/// admission. Input names and values must be admitted by their caller before
/// they are supplied to this function.
pub fn named_entries_reporting<V>(
    ctx: &DecodeContext<'_>,
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<(BTreeMap<NonBlankString, V>, Vec<NamedEntryError>), CodecError> {
    let mut kept = BTreeMap::new();
    let mut refused = Vec::new();
    for (name, value) in entries {
        ctx.charge_work(
            crate::decode::u64_from_index(name.len()),
            "named entry key scan",
        )?;
        match NonBlankString::new(name) {
            Some(key) => match kept.entry(key) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    ctx.admit_retained_btree_record::<NonBlankString, V>(
                        0,
                        "named entry map nodes",
                    )?;
                    slot.insert(value);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    let record = ctx
                        .format_retained(format_args!("{record}"), "named entry refused record")?;
                    let key = NonBlankString(
                        ctx.copy_retained_text(slot.key().as_str(), "named entry refused key")?,
                    );
                    ctx.reserve_vec(&mut refused, 1, "named entry refusals")?;
                    refused.push(NamedEntryError::Restated { record, key });
                }
            },
            None => {
                let record =
                    ctx.format_retained(format_args!("{record}"), "named entry refused record")?;
                ctx.reserve_vec(&mut refused, 1, "named entry refusals")?;
                refused.push(NamedEntryError::Blank { record });
            }
        }
    }
    Ok((kept, refused))
}

/// Keys a named set through the caller's decode budget.
///
/// Returns the first blank or restated key, or a resource refusal.
pub fn named_entries_for_decode<V>(
    ctx: &DecodeContext<'_>,
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<BTreeMap<NonBlankString, V>, NamedEntryError> {
    let (kept, refused) = named_entries_reporting(ctx, record, entries)
        .map_err(|error| NamedEntryError::from_refusal(&error))?;
    match refused.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(kept),
    }
}

/// Keys a map by the entry names, refusing a name that is blank or restated.
///
/// Codecs that read an open set of native names route the whole set through
/// here, so the rule is stated once instead of at each reader.
///
/// # Errors
///
/// Names the record that states the first key this function cannot key, and
/// for a restated key the key itself.
pub fn named_entries<V>(
    record: impl std::fmt::Display,
    entries: impl IntoIterator<Item = (String, V)>,
) -> Result<BTreeMap<NonBlankString, V>, NamedEntryError> {
    let arena = crate::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &crate::decode::DecodePolicy::default())
            .map_err(|error| NamedEntryError::from_refusal(&error))?;
    named_entries_for_decode(&ctx, record, entries)
}

/// Builds a [`NonBlankString`] from a `&'static str` constant.
///
/// [`nonblank_literal!`](crate::nonblank_literal) needs a literal template,
/// which a named constant is not. The proof is the same one: the leading byte
/// is read in the initializer of a `const` *item*, so a constant that is empty
/// or starts with whitespace or a non-ASCII byte fails `cargo check` with
/// E0080. Use it where the key is a pinned constant, so the spelling stays in
/// one place instead of being repeated as a literal beside the constant.
///
/// ```compile_fail
/// const BLANK: &str = "\x0b";
/// let _ = cadmpeg_core::nonblank_const!(BLANK);
/// ```
#[macro_export]
macro_rules! nonblank_const {
    ($constant:expr) => {{
        const NONBLANK_CONST_LEADING: $crate::text::NonWhitespaceChar = {
            let bytes = $constant.as_bytes();
            assert!(
                !bytes.is_empty(),
                "a nonblank constant must hold at least one character",
            );
            match $crate::text::NonWhitespaceChar::from_ascii(bytes[0]) {
                Some(character) => character,
                None => panic!("a nonblank literal starts with a non-whitespace ASCII character"),
            }
        };
        $crate::text::NonBlankString::prefixed(NONBLANK_CONST_LEADING, &$constant[1..])
    }};
}

impl PartialEq<str> for NonBlankString {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for NonBlankString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl std::fmt::Display for NonBlankString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NonBlankString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::custom("source identity must not be blank"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    #[cfg(feature = "schema")]
    use std::collections::BTreeMap;

    use super::{
        named_entries, named_entries_for_decode, named_entries_reporting, NamedEntryError,
        NonBlankString, NonWhitespaceChar,
    };
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn checked_reporting(
        entries: Vec<(String, i32)>,
        collection_limit: u64,
        retained_limit: u64,
    ) -> Result<
        (
            std::collections::BTreeMap<NonBlankString, i32>,
            Vec<NamedEntryError>,
        ),
        crate::CodecError,
    > {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        named_entries_reporting(&ctx, "f", entries)
    }

    #[test]
    fn nonblank_copy_refuses_retained_bytes_before_duplication() {
        let value = NonBlankString::new("abc").expect("nonblank fixture");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = value
            .try_clone_for_decode(&ctx, "nonblank copy")
            .expect_err("three bytes exceed two");
        assert!(matches!(error, crate::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "nonblank copy"));
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &service).expect("empty root");
        assert_eq!(
            value
                .try_clone_for_decode(&ctx, "nonblank copy")
                .expect("service copy"),
            value
        );
    }

    #[test]
    fn prefixes_preserve_nonblank_strings_and_wire_values() {
        const HASH: NonWhitespaceChar = match NonWhitespaceChar::from_ascii(b'#') {
            Some(character) => character,
            None => panic!("the literal is an ASCII non-whitespace character"),
        };
        const PINNED: &str = "pinned";
        for (prefix, suffix, expected) in [
            (HASH, "42", "#42"),
            (NonWhitespaceChar::hex_digit(0x0a), "", "a"),
            (NonWhitespaceChar::hex_digit(0xf0), "", "0"),
        ] {
            let value = NonBlankString::prefixed(prefix, suffix);
            assert_eq!(value.as_str(), expected);
            assert_eq!(serde_json::to_value(&value).unwrap(), expected);
        }
        assert_eq!(crate::nonblank_literal!("#{}", 42).as_str(), "#42");
        assert_eq!(crate::nonblank_const!(PINNED).as_str(), "pinned");
    }

    #[test]
    fn ascii_character_admission_is_total_over_every_byte() {
        for byte in u8::MIN..=u8::MAX {
            let admitted = NonWhitespaceChar::from_ascii(byte);
            assert_eq!(
                admitted.is_some(),
                byte.is_ascii() && !char::from(byte).is_whitespace()
            );
            if let Some(character) = admitted {
                assert_eq!(character.to_string(), char::from(byte).to_string());
            }
        }
    }

    #[test]
    fn every_admitted_prefix_produces_text_accepted_by_the_public_reader() {
        for byte in u8::MIN..=u8::MAX {
            let Some(prefix) = NonWhitespaceChar::from_ascii(byte) else {
                continue;
            };
            let value = NonBlankString::prefixed(prefix, "\t\n\u{85}\u{3000}");
            assert!(NonBlankString::new(value.as_str()).is_some(), "byte {byte}");
            let wire = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<NonBlankString>(&wire).unwrap(),
                value
            );
        }
        assert!(NonWhitespaceChar::from_ascii(0x0b).is_none());
    }

    #[cfg(feature = "schema")]
    #[test]
    fn nonblank_schema_constrains_property_names_without_trimming_source_text() {
        let schema = serde_json::to_value(schemars::schema_for!(NonBlankString)).unwrap();
        assert_eq!(schema["type"], "string");
        let pattern = schema["pattern"].as_str().unwrap();
        let map =
            serde_json::to_value(schemars::schema_for!(BTreeMap<NonBlankString, String>)).unwrap();
        assert_eq!(map["additionalProperties"], false);
        assert!(map["patternProperties"].get(pattern).is_some());
        for text in [" \tname\n", "\u{feff}", "\0"] {
            let value = NonBlankString::new(text).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), text);
        }
    }

    #[test]
    fn a_source_string_of_whitespace_alone_is_blank() {
        assert!(NonBlankString::new("   ").is_none());
        assert!(NonBlankString::new("").is_none());
        assert!(NonBlankString::new("\t\n").is_none());
        assert_eq!(
            NonBlankString::new(" a ").map(|value| value.as_str().to_owned()),
            Some(" a ".to_owned())
        );
    }

    #[test]
    fn a_blank_key_is_named_and_the_other_properties_survive() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [
            ("width".to_owned(), "10"),
            ("   ".to_owned(), "dropped"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting(&ctx, "feature 7", entries.clone()).unwrap();
        assert_eq!(refused.len(), 1);
        assert!(
            matches!(&refused[0], super::NamedEntryError::Blank { record } if record == "feature 7")
        );
        assert_eq!(
            refused[0].to_string(),
            "feature 7 states a property with a blank key"
        );
        assert_eq!(
            kept.keys().map(NonBlankString::as_str).collect::<Vec<_>>(),
            ["depth", "width"]
        );

        let error = named_entries("feature 7", entries).unwrap_err();
        assert_eq!(error, refused[0]);
    }

    #[test]
    fn a_restated_key_is_named_and_its_second_value_is_not_kept() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [
            ("k".to_owned(), "first"),
            ("k".to_owned(), "second"),
            ("depth".to_owned(), "4"),
        ];

        let (kept, refused) = named_entries_reporting(&ctx, "feature 7", entries.clone()).unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept.get("k"), Some(&"first"));
        assert_eq!(refused.len(), 1);
        assert_eq!(
            refused[0],
            NamedEntryError::Restated {
                record: "feature 7".to_owned(),
                key: NonBlankString::new("k").unwrap(),
            }
        );
        assert_eq!(
            refused[0].to_string(),
            "feature 7 states the property k a second time"
        );

        let error = named_entries("feature 7", entries).unwrap_err();
        assert_eq!(error, refused[0]);
    }

    #[test]
    fn keys_that_all_name_something_are_kept_whole() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = [("width".to_owned(), "10"), ("depth".to_owned(), "4")];

        let kept = named_entries("feature 7", entries.clone()).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(named_entries_reporting(&ctx, "feature 7", entries)
            .unwrap()
            .1
            .is_empty());
    }

    #[test]
    fn named_entry_map_storage_refuses_before_insertion() {
        let node_bytes = 11 * (std::mem::size_of::<NonBlankString>() + std::mem::size_of::<i32>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<NonBlankString>().max(std::mem::align_of::<usize>());
        // One named-entry record admits one backing node before insertion.
        let bytes = node_bytes;
        let error = checked_reporting(
            vec![("k".into(), 1)],
            1,
            crate::decode::u64_from_index(bytes) - 1,
        )
        .unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "named entry map nodes"
                && limit.additional == crate::decode::u64_from_index(bytes)));
    }

    #[test]
    fn named_entry_refusal_storage_refuses_before_vector_growth() {
        let bytes = std::mem::size_of::<NamedEntryError>();
        let error = checked_reporting(
            vec![(" ".into(), 1)],
            1,
            crate::decode::u64_from_index(bytes),
        )
        .unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "named entry refusals"
                && limit.additional == crate::decode::u64_from_index(4 * bytes)));
    }

    #[test]
    fn named_entry_scan_refuses_work_before_map_insertion() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = named_entries_reporting(&ctx, "f", [("width".into(), 1)]).unwrap_err();
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "named entry key scan"));
    }

    #[test]
    fn named_entry_strict_resource_refusal_preserves_dimension() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = named_entries_for_decode(&ctx, "f", [("width".into(), 1)]).unwrap_err();
        assert!(
            matches!(crate::CodecError::from(error), crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "named entry map nodes")
        );
    }

    #[test]
    fn checked_named_entry_map_refuses_before_btree_insertion() {
        let entries = vec![("width".to_owned(), 1)];
        assert_eq!(
            checked_reporting(entries.clone(), 1, 1000).unwrap().0.len(),
            1
        );
        assert!(matches!(
            checked_reporting(entries, 0, 1000),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "named entry map nodes"
        ));
    }

    #[test]
    fn checked_named_entry_refusals_refuse_before_vec_growth() {
        let entries = vec![("width".to_owned(), 1), ("width".to_owned(), 2)];
        assert_eq!(
            checked_reporting(entries.clone(), 2, 10_000)
                .unwrap()
                .1
                .len(),
            1
        );
        assert!(matches!(
            checked_reporting(entries, 1, 10_000),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "named entry refusals"
        ));
    }

    #[test]
    fn checked_named_entry_record_refuses_before_text_growth() {
        let entries = vec![(" ".to_owned(), 1)];
        assert!(matches!(
            checked_reporting(entries, 10, 0),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "named entry refused record"
        ));
    }

    #[test]
    fn checked_named_entry_restated_key_refuses_before_text_growth() {
        let entries = vec![("width".to_owned(), 1), ("width".to_owned(), 2)];
        // One backing node and one record byte precede the restated key copy.
        let node_bytes = 11 * (std::mem::size_of::<NonBlankString>() + std::mem::size_of::<i32>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<NonBlankString>();
        assert!(matches!(
            checked_reporting(entries, 10, crate::decode::u64_from_index(node_bytes) + 1),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "named entry refused key"
        ));
    }

    #[test]
    fn checked_named_entries_keep_order_and_first_value() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let entries = vec![
            ("width".to_owned(), 1),
            ("width".to_owned(), 2),
            ("depth".to_owned(), 3),
        ];
        let (kept, refused) = checked_reporting(entries.clone(), 10, 10_000).unwrap();
        assert_eq!(kept.get("width"), Some(&1));
        assert_eq!(
            kept.keys().map(NonBlankString::as_str).collect::<Vec<_>>(),
            ["depth", "width"]
        );
        assert_eq!(
            refused,
            named_entries_reporting(&ctx, "f", entries).unwrap().1
        );

        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            named_entries_for_decode(&ctx, "f", [(" ".to_owned(), 1)])
                .map_err(crate::CodecError::from),
            Err(crate::CodecError::Malformed(_))
        ));
    }
}
