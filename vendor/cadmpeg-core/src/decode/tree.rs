// SPDX-License-Identifier: Apache-2.0
//! Admission of parser-owned XML and JSON trees.

use super::{u64_from_index, DecodeContext, ScopedReservation};
use crate::CodecError;

/// Bounds roxmltree 0.21.1 `NodeData`, including `NodeKind`, four node IDs,
/// positions (`Range<usize>`), enum alignment and inline expanded names.
const NODE_RECORD_BOUND: u64 = 128;
/// Bounds roxmltree 0.21.1 `AttributeData` and `TempAttributeData`, including
/// expanded names, `StringStorage`, position ranges and alignment.
const ATTRIBUTE_RECORD_BOUND: u64 = 128;
/// Bounds roxmltree 0.21.1 `Namespace`, including optional prefix,
/// `StringStorage` and alignment. Namespace indices are admitted separately.
const NAMESPACE_RECORD_BOUND: u64 = 64;
// The built-in xml prefix and URI participate in expanded-name comparisons.
const XML_PREFIX_BYTES: u64 = 3;
const XML_NAMESPACE_URI_BYTES: u64 = 36;

/// An XML document and the admission held until its storage is released.
#[derive(Debug)]
pub struct AdmittedXml<'input, 'ctx> {
    document: roxmltree::Document<'input>,
    _reservation: ScopedReservation<'ctx>,
}

impl<'input> AdmittedXml<'input, '_> {
    /// Borrows the admitted tree without releasing its reservation.
    pub fn document(&self) -> &roxmltree::Document<'input> {
        &self.document
    }
}

#[derive(Clone, Copy)]
enum XmlScan<'input> {
    Text,
    Tag(XmlTag<'input>),
    Comment,
    Cdata,
    Instruction,
    Declaration,
}

#[derive(Clone, Copy)]
struct XmlTag<'input> {
    closing: bool,
    quote: u8,
    last: u8,
    attributes: u64,
    name_bytes: u64,
    name_complete: bool,
    attribute_name_bytes: u64,
    namespace_key_bytes: u64,
    prefixed_attributes: u64,
    namespaces: u64,
    start: usize,
    undo_start: usize,
    namespace_prefix: Option<&'input str>,
    quote_start: usize,
}

/// Upper bounds for the namespace list visible at one element.
#[derive(Clone, Copy)]
struct XmlNamespaceScope {
    element_depth: u64,
    undo_start: usize,
    count: u64,
    max_prefix: u64,
    max_uri: u64,
}

impl Default for XmlNamespaceScope {
    fn default() -> Self {
        Self {
            element_depth: 0,
            undo_start: 0,
            count: 0,
            max_prefix: XML_PREFIX_BYTES,
            max_uri: XML_NAMESPACE_URI_BYTES,
        }
    }
}

/// Borrow prefix text and restore bindings on closing tags. Namespace-free
/// children reuse their parent's bound; declarations recompute the union.
struct XmlNamespaces<'ctx, 'input> {
    prefixes: std::collections::HashMap<&'input str, u64>,
    bindings: std::collections::HashSet<(&'input str, &'input str)>,
    undo: Vec<(&'input str, Option<u64>)>,
    scopes: Vec<XmlNamespaceScope>,
    storage: ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'ctx>,
}

impl<'ctx, 'input> XmlNamespaces<'ctx, 'input> {
    fn new(ctx: &'ctx DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        Ok(Self {
            prefixes: std::collections::HashMap::new(),
            bindings: std::collections::HashSet::new(),
            undo: Vec::new(),
            scopes: Vec::new(),
            storage: ctx.reserve_scoped(0, operation)?,
            ctx,
        })
    }

    fn parent(&self) -> XmlNamespaceScope {
        self.scopes.last().copied().unwrap_or_default()
    }

    fn declare(&mut self, prefix: &'input str, operation: &'static str) -> Result<(), CodecError> {
        self.ctx.charge_work(
            u64_from_index(prefix.len())
                .checked_mul(6)
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| self.ctx.tree_overflow(operation))?,
            operation,
        )?;
        let previous = self.prefixes.get(prefix).copied();
        if previous.is_none() && self.prefixes.len() == self.prefixes.capacity() {
            for stored in self.prefixes.keys() {
                self.ctx.charge_work(
                    u64_from_index(stored.len())
                        .checked_add(1)
                        .ok_or_else(|| self.ctx.tree_overflow(operation))?,
                    operation,
                )?;
            }
        }
        self.storage.with_storage(|| {
            self.ctx
                .push_vec(&mut self.undo, (prefix, previous), operation)?;
            self.ctx
                .insert_hash_map(&mut self.prefixes, prefix, 0, operation)?;
            Ok::<_, CodecError>(())
        })
    }

    fn value(
        &mut self,
        prefix: &'input str,
        uri: &'input str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.ctx.charge_work(
            u64_from_index(prefix.len())
                .checked_mul(4)
                .and_then(|work| work.checked_add(u64_from_index(uri.len()).checked_mul(2)?))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| self.ctx.tree_overflow(operation))?,
            operation,
        )?;
        if let Some(value) = self.prefixes.get_mut(prefix) {
            *value = u64_from_index(uri.len());
        }
        if self.bindings.contains(&(prefix, uri)) {
            return Ok(());
        }
        if self.bindings.len() == self.bindings.capacity() {
            for (stored_prefix, stored_uri) in &self.bindings {
                self.ctx.charge_work(
                    u64_from_index(stored_prefix.len())
                        .checked_add(u64_from_index(stored_uri.len()))
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| self.ctx.tree_overflow(operation))?,
                    operation,
                )?;
            }
        }
        self.storage.with_storage(|| {
            self.ctx.reserve_set(&mut self.bindings, 1, operation)?;
            self.bindings.insert((prefix, uri));
            Ok::<_, CodecError>(())
        })
    }

    fn scope(
        &self,
        tag: &XmlTag<'_>,
        operation: &'static str,
    ) -> Result<XmlNamespaceScope, CodecError> {
        let mut scope = self.parent();
        scope.undo_start = tag.undo_start;
        if tag.namespaces != 0 {
            scope = XmlNamespaceScope {
                undo_start: tag.undo_start,
                // Local declarations include repeated default namespace bindings,
                // which roxmltree can retain even when their prefixes are equal.
                count: u64_from_index(self.prefixes.len())
                    .checked_add(tag.namespaces)
                    .ok_or_else(|| self.ctx.tree_overflow(operation))?,
                ..XmlNamespaceScope::default()
            };
            for (prefix, uri_bytes) in &self.prefixes {
                self.ctx.charge_work(1, operation)?;
                scope.max_prefix = scope.max_prefix.max(u64_from_index(prefix.len()));
                scope.max_uri = scope.max_uri.max(*uri_bytes);
            }
        }
        Ok(scope)
    }

    fn restore(&mut self, start: usize, operation: &'static str) -> Result<(), CodecError> {
        while self.undo.len() > start {
            if let Some((prefix, previous)) = self.undo.pop() {
                self.ctx.charge_work(
                    u64_from_index(prefix.len())
                        .checked_mul(2)
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| self.ctx.tree_overflow(operation))?,
                    operation,
                )?;
                match previous {
                    Some(previous) => {
                        self.prefixes.insert(prefix, previous);
                    }
                    None => {
                        self.prefixes.remove(prefix);
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(
        &mut self,
        tag: &XmlTag<'_>,
        mut scope: XmlNamespaceScope,
        depth: Option<u64>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if tag.closing {
            if self
                .scopes
                .last()
                .is_some_and(|parent| Some(parent.element_depth) == depth)
            {
                if let Some(parent) = self.scopes.pop() {
                    self.restore(parent.undo_start, operation)?;
                }
            }
        } else if tag.last == b'/' {
            self.restore(tag.undo_start, operation)?;
        } else if tag.namespaces != 0 {
            scope.element_depth = depth
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| self.ctx.tree_overflow(operation))?;
            self.storage
                .with_storage(|| self.ctx.push_vec(&mut self.scopes, scope, operation))?;
        }
        Ok(())
    }
}

struct XmlBound {
    nodes: u64,
    attributes: u64,
    namespace_references: Option<u64>,
    depth: u64,
    local_comparisons: Option<u64>,
    namespace_unique: u64,
    namespace_bytes: u64,
}

impl XmlBound {
    fn tag(&mut self, tag: &XmlTag<'_>, parent: XmlNamespaceScope, scope: XmlNamespaceScope) {
        self.namespace_bytes += tag.namespace_key_bytes;
        if tag.namespaces != 0 {
            self.namespace_references = self
                .namespace_references
                .and_then(|references| references.checked_add(scope.count));
        }
        self.local_comparisons = self.local_comparisons.and_then(|work| {
            let attributes = tag
                .attribute_name_bytes
                .checked_add(tag.attributes)?
                .checked_mul(tag.attributes.checked_add(1)?)?;
            let lookups = tag
                .name_bytes
                .checked_add(tag.attribute_name_bytes)?
                .checked_add(tag.attributes.checked_add(1)?)?
                .checked_mul(scope.count.checked_add(1)?)?;
            let uris = tag
                .prefixed_attributes
                .checked_mul(tag.prefixed_attributes)?
                .checked_mul(scope.max_uri.checked_add(1)?)?;
            let namespaces = if tag.namespaces == 0 {
                0
            } else {
                // Namespace inheritance scans parent names against the growing
                // local list. Duplicate declarations compare local names too.
                parent
                    .count
                    .checked_mul(parent.count.checked_add(tag.namespaces)?)?
                    .checked_add(tag.namespaces.checked_mul(tag.namespaces)?)?
                    .checked_mul(scope.max_prefix.checked_add(1)?)?
            };
            work.checked_add(attributes)?
                .checked_add(lookups)?
                .checked_add(uris)?
                .checked_add(namespaces)
        });
    }

    fn comparison_work(&self) -> Option<u64> {
        let namespaces = self.namespace_unique.checked_add(1)?;
        let levels = u64::from(u64::BITS - namespaces.leading_zeros()) + 1;
        // Namespace deduplication binary-searches string pairs, then inserts
        // u16 indices into a sorted vector. Only insertion moves are quadratic.
        let insertion = self
            .namespace_bytes
            .checked_mul(levels)?
            .checked_add(namespaces.checked_mul(namespaces)?.checked_mul(2)?)?;
        self.local_comparisons?.checked_add(insertion)
    }
}

/// Counts delimiters, element depth and local comparisons in one pass.
/// Quoted values, comments, CDATA and instructions do not change nesting.
fn xml_bound(
    text: &str,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<XmlBound, CodecError> {
    let bytes = text.as_bytes();
    let mut depth = Some(0_u64);
    let mut maximum = 0;
    let mut bound = XmlBound {
        nodes: 0,
        attributes: 0,
        namespace_references: Some(0),
        depth: 0,
        local_comparisons: Some(0),
        namespace_unique: 0,
        namespace_bytes: 0,
    };
    let mut namespaces = XmlNamespaces::new(ctx, operation)?;
    let mut state = XmlScan::Text;
    for (index, &byte) in bytes.iter().enumerate() {
        bound.nodes += u64::from(byte == b'<');
        bound.attributes += u64::from(byte == b'=');
        let tail = &bytes[index..];
        state = match state {
            XmlScan::Text if byte == b'<' => {
                if tail.starts_with(b"<!--") {
                    XmlScan::Comment
                } else if tail.starts_with(b"<![CDATA[") {
                    XmlScan::Cdata
                } else if tail.starts_with(b"<?") {
                    XmlScan::Instruction
                } else if tail.starts_with(b"<!") {
                    XmlScan::Declaration
                } else {
                    XmlScan::Tag(XmlTag {
                        closing: tail.starts_with(b"</"),
                        quote: 0,
                        last: byte,
                        attributes: 0,
                        name_bytes: 0,
                        name_complete: false,
                        attribute_name_bytes: 0,
                        namespace_key_bytes: 0,
                        prefixed_attributes: 0,
                        namespaces: 0,
                        start: index,
                        undo_start: namespaces.undo.len(),
                        namespace_prefix: None,
                        quote_start: index,
                    })
                }
            }
            XmlScan::Tag(mut tag) => {
                // The slash before a closing QName is not part of its key.
                if !(tag.name_complete || byte == b'/' && tag.closing && tag.name_bytes == 0) {
                    if byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>') {
                        tag.name_complete = true;
                    } else {
                        tag.name_bytes += 1;
                    }
                }
                if tag.quote != 0 {
                    if byte == tag.quote {
                        if let Some(prefix) = tag.namespace_prefix {
                            tag.namespace_key_bytes = tag
                                .namespace_key_bytes
                                .checked_add(u64_from_index(index - tag.quote_start))
                                .ok_or_else(|| ctx.tree_overflow(operation))?;
                            namespaces.value(prefix, &text[tag.quote_start..index], operation)?;
                        }
                        tag.quote = 0;
                    }
                } else if matches!(byte, b'\'' | b'"') {
                    tag.quote = byte;
                    tag.quote_start = index + 1;
                } else if byte == b'>' {
                    let scope = namespaces.scope(&tag, operation)?;
                    bound.tag(&tag, namespaces.parent(), scope);
                    namespaces.finish(&tag, scope, depth, operation)?;
                    if tag.closing {
                        depth = depth.and_then(|depth| depth.checked_sub(1));
                    } else if let Some(parent_depth) = depth {
                        let element_depth = parent_depth + 1;
                        maximum = maximum.max(element_depth);
                        if tag.last != b'/' {
                            depth = Some(element_depth);
                        }
                    }
                    state = XmlScan::Text;
                    continue;
                } else {
                    if byte == b'=' {
                        let mut end = index;
                        while end > tag.start && bytes[end - 1].is_ascii_whitespace() {
                            end -= 1;
                        }
                        let mut begin = end;
                        while begin > tag.start
                            && !bytes[begin - 1].is_ascii_whitespace()
                            && !matches!(bytes[begin - 1], b'<' | b'>' | b'=' | b'\'' | b'"' | b'/')
                        {
                            begin -= 1;
                        }
                        let name = &text[begin..end];
                        tag.attribute_name_bytes = tag
                            .attribute_name_bytes
                            .checked_add(u64_from_index(name.len()))
                            .ok_or_else(|| ctx.tree_overflow(operation))?;
                        tag.namespace_prefix = if name == "xmlns" {
                            Some("")
                        } else {
                            name.strip_prefix("xmlns:")
                        };
                        if let Some(prefix) = tag.namespace_prefix {
                            tag.namespaces += 1;
                            tag.namespace_key_bytes = tag
                                .namespace_key_bytes
                                .checked_add(u64_from_index(name.len()))
                                .ok_or_else(|| ctx.tree_overflow(operation))?;
                            namespaces.declare(prefix, operation)?;
                        } else {
                            tag.prefixed_attributes += u64::from(name.contains(':'));
                        }
                        tag.attributes += 1;
                    }
                    tag.last = byte;
                }
                XmlScan::Tag(tag)
            }
            XmlScan::Comment if tail.starts_with(b"-->") => XmlScan::Text,
            XmlScan::Cdata if tail.starts_with(b"]]>") => XmlScan::Text,
            XmlScan::Instruction if tail.starts_with(b"?>") => XmlScan::Text,
            XmlScan::Declaration if byte == b'>' => XmlScan::Text,
            other => other,
        };
    }
    if let XmlScan::Tag(tag) = state {
        let scope = namespaces.scope(&tag, operation)?;
        bound.tag(&tag, namespaces.parent(), scope);
    }
    bound.depth = maximum;
    bound.namespace_unique = u64_from_index(namespaces.bindings.len());
    Ok(bound)
}

/// Holds nesting guards through the parser call without recursing in the
/// admission code. Guard slots and their temporary storage are admitted first.
fn at_depth<T>(
    ctx: &DecodeContext<'_>,
    depth: u64,
    operation: &'static str,
    parse: impl FnOnce() -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    if depth == 0 {
        return parse();
    }
    ctx.charge_work(depth, operation)?;
    ctx.charge_collection_items(depth, operation)?;
    let count =
        usize::try_from(depth).map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX, depth))?;
    let capacity = count
        .checked_add(4)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, depth))?;
    let (_reservation, mut guards) = {
        let (guards, reservation) = ctx.scoped_vector_storage(capacity, operation)?;
        (reservation, guards)
    };
    for _ in 0..count {
        guards.push(ctx.enter_nested(operation)?);
    }
    let result = parse();
    drop(guards);
    result
}

impl DecodeContext<'_> {
    fn tree_malformed(&self, error: impl std::fmt::Display, operation: &'static str) -> CodecError {
        match self.format_retained(format_args!("{error}"), operation) {
            Ok(message) => CodecError::Malformed(message),
            Err(error) => error,
        }
    }

    fn tree_overflow(&self, operation: &'static str) -> CodecError {
        self.refuse_codec_limit(operation, u64::MAX, u64::MAX)
    }

    /// Admits roxmltree 0.21.1 storage before parsing with DTD disabled.
    /// For N = 2 * '<' + 2, A = '=' and S = distinct raw namespace bindings + 1,
    /// growing vectors reserve 2 * count + their minimum capacity. Storage covers
    /// nodes, permanent and temporary attributes (minimum 16), namespace records
    /// and sorted indices. Namespace tree-order slots sum visible bindings at
    /// elements with declarations; other elements reuse their parent's range.
    /// Index storage includes old and new allocations during vector growth.
    /// Node-sized scratch includes awaiting-subtree IDs, parent prefixes and text Cows.
    /// Eight input lengths cover normalization buffers, joined text, shared
    /// strings and transient copies; 32 bytes per string cover Arc headers.
    /// The fixed 1024 bytes cover initial scratch capacities and parser state.
    /// Work covers the scan, parser scans and quadratic attribute/namespace
    /// comparisons, including string comparisons and namespace insertion.
    /// Attribute comparisons use each tag's name bytes and attribute count. Namespace
    /// work covers prefix lookups, expanded attribute names, inheritance only
    /// at tags with declarations, and sorted namespace insertion. Text outside
    /// tags and ordinary attribute values pay only scanning/normalization work.
    /// Temporary node-ID, prefix,
    /// text, attribute and namespace-index slots are admitted separately.
    pub fn parse_xml<'input>(
        &self,
        text: &'input str,
        operation: &'static str,
    ) -> Result<AdmittedXml<'input, '_>, CodecError> {
        let length = u64_from_index(text.len());
        self.charge_work(length, operation)?;
        let bound = xml_bound(text, self, operation)?;
        let nodes = bound
            .nodes
            .checked_mul(2)
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let namespaces = bound
            .namespace_unique
            .checked_add(1)
            .ok_or_else(|| self.tree_overflow(operation))?;
        let namespace_references = bound
            .namespace_references
            .ok_or_else(|| self.tree_overflow(operation))?;
        let capacity = |n: u64, minimum: u64| n.checked_mul(2).and_then(|n| n.checked_add(minimum));
        let bytes = (|| {
            let node_bytes = capacity(nodes, 4)?.checked_mul(NODE_RECORD_BOUND + 64)?;
            let attribute_bytes =
                capacity(bound.attributes, 16)?.checked_mul(ATTRIBUTE_RECORD_BOUND * 2)?;
            let namespace_bytes =
                capacity(namespaces, 4)?.checked_mul(NAMESPACE_RECORD_BOUND + 2)?;
            let namespace_indices = namespace_references
                .checked_mul(3)?
                .checked_add(4)?
                .checked_mul(2)?;
            let strings = nodes
                .checked_add(bound.attributes)?
                .checked_add(namespaces)?
                .checked_mul(32)?;
            node_bytes
                .checked_add(attribute_bytes)?
                .checked_add(namespace_bytes)?
                .checked_add(namespace_indices)?
                .checked_add(strings)?
                .checked_add(length.checked_mul(8)?)?
                .checked_add(1024)
        })()
        .ok_or_else(|| self.tree_overflow(operation))?;
        let work = length
            .checked_mul(4)
            .and_then(|n| n.checked_add(nodes))
            .and_then(|n| n.checked_add(bound.comparison_work()?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_collection_items(nodes, operation)?;
        self.charge_collection_items(bound.attributes, operation)?;
        self.charge_collection_items(namespaces, operation)?;
        let scratch_items = nodes
            .checked_mul(3)
            .and_then(|n| n.checked_add(bound.attributes))
            .and_then(|n| n.checked_add(namespaces))
            .and_then(|n| n.checked_add(namespace_references))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_collection_items(scratch_items, operation)?;
        self.charge_work(work, operation)?;
        let reservation = self.reserve_scoped(bytes, operation)?;
        let nodes_limit = u32::try_from(nodes).map_err(|_| self.tree_overflow(operation))?;
        let document = at_depth(self, bound.depth, operation, || {
            roxmltree::Document::parse_with_options(
                text,
                roxmltree::ParsingOptions {
                    nodes_limit,
                    ..roxmltree::ParsingOptions::default()
                },
            )
            .map_err(|error| match error {
                roxmltree::Error::NodesLimitReached => {
                    self.refuse_codec_limit(operation, u64::from(nodes_limit), nodes)
                }
                other => self.tree_malformed(other, operation),
            })
        })?;
        Ok(AdmittedXml {
            document,
            _reservation: reservation,
        })
    }
}

/// `serde_json` 1.0.151 uses a `BTreeMap` without `preserve_order`. A leaf has
/// eleven (String, Value) slots; an internal node adds twelve pointers.
/// 4096 bytes per potential entry cover two nodes, split/root growth and
/// alignment, including a one-entry root where capacity rounding dominates.
const JSON_MAP_ENTRY_BOUND: u64 = 4096;
const RAW_VALUE_TOKEN: &[u8] = b"$serde_json::private::RawValue";

#[derive(Default)]
struct JsonStringScan {
    active: bool,
    escaped: bool,
    unicode_digits: u8,
    unicode_value: u32,
    matched: usize,
    possible: bool,
}

impl JsonStringScan {
    fn decoded(&mut self, byte: Option<u8>) {
        if byte.is_some() && byte == RAW_VALUE_TOKEN.get(self.matched).copied() {
            self.matched += 1;
        } else {
            self.possible = false;
        }
    }

    /// Returns true only when a completed string spells the raw-value token.
    fn byte(&mut self, byte: u8) -> bool {
        if self.unicode_digits != 0 {
            if let Some(digit) = char::from(byte).to_digit(16) {
                self.unicode_value = self.unicode_value * 16 + digit;
            } else {
                self.possible = false;
            }
            self.unicode_digits -= 1;
            if self.unicode_digits == 0 {
                self.decoded(u8::try_from(self.unicode_value).ok());
            }
        } else if self.escaped {
            self.escaped = false;
            if byte == b'u' {
                self.unicode_digits = 4;
                self.unicode_value = 0;
            } else {
                self.decoded(match byte {
                    b'"' | b'\\' | b'/' => Some(byte),
                    b'b' => Some(8),
                    b'f' => Some(12),
                    b'n' => Some(b'\n'),
                    b'r' => Some(b'\r'),
                    b't' => Some(b'\t'),
                    _ => None,
                });
            }
        } else if byte == b'\\' {
            self.escaped = true;
        } else if byte == b'"' {
            self.active = false;
            return self.possible && self.matched == RAW_VALUE_TOKEN.len();
        } else {
            self.decoded(Some(byte));
        }
        false
    }
}

/// Builds an ordinary JSON tree for derived typed conversion. The Value
/// deserializer's private raw-value carrier is not part of derived JSON data;
/// ignored extension members must remain ordinary objects during admission.
struct PlainJson;

impl<'de> serde::de::DeserializeSeed<'de> for PlainJson {
    type Value = serde_json::Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, parser: D) -> Result<Self::Value, D::Error> {
        parser.deserialize_any(self)
    }
}

impl<'de> serde::de::Visitor<'de> for PlainJson {
    type Value = serde_json::Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value.to_owned()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(PlainJson)? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(PlainJson)?;
            drop(values.insert(key, value));
        }
        Ok(serde_json::Value::Object(values))
    }
}

struct JsonBound {
    values: u64,
    entries: u64,
    depth: u64,
    bytes: u64,
}

impl DecodeContext<'_> {
    /// Bounds `serde_json` 1.0.151 trees in one scan. Delimiters '[' '{' ',' ':'
    /// plus one bound value slots. All array capacities together are at most
    /// six slots per value (doubling plus minimum four). Map entry capacity is
    /// charged per colon. Eight input lengths cover decoded
    /// strings, growing scratch and temporary copies; 1024 covers parser state.
    /// The `raw_value` feature can parse strings again when a first key spells
    /// its private token, including escaped spellings. For that carrier,
    /// opening delimiters and escapes bound replay depth and hidden delimiters;
    /// eight input lengths per level cover simultaneous replay strings/scratch.
    /// Work includes replay scans, map comparisons and copies before parsing.
    fn json_bound(&self, text: &str, operation: &'static str) -> Result<JsonBound, CodecError> {
        let length = u64_from_index(text.len());
        self.charge_work(length, operation)?;
        let mut values = 0_u64;
        let mut entries = 0_u64;
        let mut depth = 0_u64;
        let mut maximum = 0_u64;
        let mut openings = 0_u64;
        let mut escapes = 0_u64;
        let mut raw = false;
        let mut string = JsonStringScan::default();
        for &byte in text.as_bytes() {
            values += u64::from(matches!(byte, b'[' | b'{' | b',' | b':'));
            entries += u64::from(byte == b':');
            openings += u64::from(matches!(byte, b'[' | b'{'));
            escapes += u64::from(byte == b'\\');
            if string.active {
                raw |= string.byte(byte);
                continue;
            }
            match byte {
                b'"' => {
                    string = JsonStringScan {
                        active: true,
                        possible: true,
                        ..JsonStringScan::default()
                    }
                }
                b'[' | b'{' => {
                    depth += 1;
                    maximum = maximum.max(depth);
                }
                b']' | b'}' if depth != 0 => depth -= 1,
                _ => {}
            }
        }
        values = values
            .checked_add(1)
            .ok_or_else(|| self.tree_overflow(operation))?;
        let levels = if raw {
            maximum = openings
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            values = values
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            entries = entries
                .checked_add(escapes)
                .ok_or_else(|| self.tree_overflow(operation))?;
            maximum
                .checked_add(1)
                .ok_or_else(|| self.tree_overflow(operation))?
        } else {
            1
        };
        let bytes = (|| {
            values
                .checked_mul(6)?
                .checked_mul(u64_from_index(std::mem::size_of::<serde_json::Value>()))?
                .checked_add(entries.checked_mul(JSON_MAP_ENTRY_BOUND)?)?
                .checked_add(length.checked_mul(8)?.checked_mul(levels)?)?
                .checked_add(1024)
        })()
        .ok_or_else(|| self.tree_overflow(operation))?;
        Ok(JsonBound {
            values,
            entries,
            depth: maximum,
            bytes,
        })
    }

    fn parse_json_tree(
        &self,
        text: &str,
        operation: &'static str,
        interpret_raw: bool,
    ) -> Result<(serde_json::Value, ScopedReservation<'_>, JsonBound), CodecError> {
        let bound = self.json_bound(text, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        let work = u64_from_index(text.len())
            .checked_mul(
                bound
                    .entries
                    .checked_add(1)
                    .ok_or_else(|| self.tree_overflow(operation))?,
            )
            .and_then(|n| n.checked_mul(bound.depth.checked_add(1)?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_work(work, operation)?;
        let reservation = self.reserve_scoped(bound.bytes, operation)?;
        let value = at_depth(self, bound.depth, operation, || {
            if interpret_raw {
                serde_json::from_str(text).map_err(|error| self.tree_malformed(error, operation))
            } else {
                use serde::de::DeserializeSeed;
                let mut parser = serde_json::Deserializer::from_str(text);
                PlainJson
                    .deserialize(&mut parser)
                    .and_then(|value| {
                        parser.end()?;
                        Ok(value)
                    })
                    .map_err(|error| self.tree_malformed(error, operation))
            }
        })?;
        Ok((value, reservation, bound))
    }

    /// Parses a value tree under collection, work, scoped storage and depth
    /// admission. Keep the returned reservation alive with the returned Value.
    /// `serde_json`'s own 128-level recursion ceiling also applies.
    pub fn parse_json_value(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<(serde_json::Value, ScopedReservation<'_>), CodecError> {
        let (value, reservation, _) = self.parse_json_tree(text, operation, true)?;
        Ok((value, reservation))
    }

    /// Parses types with derived Deserialize: structs, enums, vectors, maps,
    /// strings and scalars. Custom allocating deserializers are outside this
    /// bound. The retained typed result is admitted as twice the value-tree
    /// storage plus `size_of::<T>()`; conversion charges one work unit per value.
    /// Source validation with the derived deserializer preserves duplicate-field
    /// errors that Value maps erase. Its temporary result has a separate scoped
    /// admission of twice the tree bytes plus `size_of::<T>()`; scan and conversion
    /// work are charged before validation. Typed trees use ordinary map members
    /// rather than Value's private raw-value carrier. The admission remains live.
    pub fn parse_json<T: serde::de::DeserializeOwned>(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<T, CodecError> {
        let (_reservation, value, bound) = {
            let (value, reservation, bound) = self.parse_json_tree(text, operation, false)?;
            (reservation, value, bound)
        };
        let retained = bound
            .bytes
            .checked_mul(2)
            .and_then(|n| n.checked_add(u64_from_index(std::mem::size_of::<T>())))
            .ok_or_else(|| self.tree_overflow(operation))?;
        let validation_work = u64_from_index(text.len())
            .checked_mul(bound.values)
            .and_then(|n| n.checked_mul(bound.depth.checked_add(1)?))
            .ok_or_else(|| self.tree_overflow(operation))?;
        self.charge_work(validation_work, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        {
            let _validation = self.reserve_scoped(retained, operation)?;
            at_depth(self, bound.depth, operation, || {
                let validated: T = serde_json::from_str(text)
                    .map_err(|error| self.tree_malformed(error, operation))?;
                drop(validated);
                Ok(())
            })?;
        }
        self.charge_retained(retained, operation)?;
        self.charge_collection_items(bound.values, operation)?;
        self.charge_work(bound.values, operation)?;
        at_depth(self, bound.depth, operation, || {
            serde_json::from_value(value).map_err(|error| self.tree_malformed(error, operation))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ATTRIBUTE_RECORD_BOUND;
    use crate::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::CodecError;

    #[test]
    fn tree_xml_success_and_capacity_rounding() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let tree = ctx.parse_xml("<r/>", "XML tree").expect("valid fixture");
        assert_eq!(tree.document().root_element().tag_name().name(), "r");
        let mut policy = DecodePolicy::default();
        policy.limits.max_materialized_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let CodecError::ResourceLimit(limit) = limited
            .parse_xml("<r/>", "XML tree")
            .expect_err("fixture must refuse")
        else {
            panic!("byte refusal");
        };
        assert!(limit.additional >= 16 * ATTRIBUTE_RECORD_BOUND);
    }

    #[test]
    fn tree_xml_work_compares_attributes_within_each_tag() {
        let text = format!("<r>{}</r>", "<a x='0'/>".repeat(16));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 32 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let tree = ctx.parse_xml(&text, "XML tree").expect("valid fixture");
        assert_eq!(tree.document().root_element().children().count(), 16);
    }

    #[test]
    fn tree_xml_large_text_does_not_pay_attribute_comparison_work() {
        let mut text = String::from("<r");
        for index in 0..64 {
            std::fmt::Write::write_fmt(&mut text, format_args!(" a{index}='0'"))
                .expect("fixture string");
        }
        text.push('>');
        text.push_str(&"ordinary text ".repeat(16_000));
        text.push_str("</r>");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let tree = ctx
            .parse_xml(&text, "local XML comparisons")
            .expect("text scanning is linear");
        assert_eq!(tree.document().root_element().attributes().len(), 64);
        drop(tree);
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_large_attribute_values_do_not_pay_key_comparison_work() {
        let value = "value text ".repeat(16_000);
        let mut text = String::from("<r xmlns:p='urn:shared'");
        for index in 0..64 {
            std::fmt::Write::write_fmt(&mut text, format_args!(" a{index}='0'"))
                .expect("fixture string");
        }
        std::fmt::Write::write_fmt(&mut text, format_args!(" data='{value}'/>"))
            .expect("fixture string");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let tree = ctx
            .parse_xml(&text, "XML attribute value scanning")
            .expect("values are not comparison keys");
        assert_eq!(tree.document().root_element().attributes().len(), 65);
        assert_eq!(
            tree.document().root_element().attribute("data"),
            Some(value.as_str())
        );
        drop(tree);
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_namespace_comparisons_are_separate_from_text() {
        let text = format!(
            "<r xmlns:p='urn:shared' xmlns:q='urn:shared'><s p:a='1' q:b='2'>{}</s></r>",
            "text ".repeat(8_000)
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 8 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let tree = ctx
            .parse_xml(&text, "namespace XML comparisons")
            .expect("local namespaces");
        let child = tree
            .document()
            .root_element()
            .first_element_child()
            .expect("s element");
        assert_eq!(child.attribute(("urn:shared", "a")), Some("1"));
        assert_eq!(child.attribute(("urn:shared", "b")), Some("2"));
        drop(tree);
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_repeated_sibling_namespaces_use_local_uri_bounds() {
        let text = format!(
            "<r><a xmlns:p='{}'/>{}</r>",
            "u".repeat(8_192),
            "<b xmlns:p='urn:small'><p:t p:x='1' p:y='2'/></b>".repeat(128)
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 32 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let tree = ctx
            .parse_xml(&text, "scoped XML namespaces")
            .expect("sibling namespace work is local");
        assert_eq!(
            tree.document()
                .root_element()
                .children()
                .filter(roxmltree::Node::is_element)
                .count(),
            129
        );
        let last = tree
            .document()
            .root_element()
            .last_element_child()
            .expect("b element");
        let child = last.first_element_child().expect("p:t element");
        assert_eq!(child.attribute(("urn:small", "x")), Some("1"));
        drop(tree);
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_sibling_namespace_storage_is_linear_in_declarations() {
        let text = format!(
            "<r>{}</r>",
            "<b xmlns:p='urn:shared'><p:t p:x='1'/></b>".repeat(5_000)
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 200_000;
        policy.limits.max_materialized_bytes = 32 * 1024 * 1024;
        policy.limits.max_work_units = 16 * u64_from_index(text.len());
        let (ctx, _) =
            DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy).expect("XML root");
        let tree = ctx
            .parse_xml(&text, "linear XML namespace storage")
            .expect("sibling namespace ranges fit the collection and storage limits");
        assert_eq!(
            tree.document()
                .root_element()
                .children()
                .filter(roxmltree::Node::is_element)
                .count(),
            5_000
        );
        let last = tree
            .document()
            .root_element()
            .last_element_child()
            .expect("b element");
        assert_eq!(
            last.first_element_child()
                .expect("p:t element")
                .attribute(("urn:shared", "x")),
            Some("1")
        );
        drop(tree);
        drop(
            ctx.reserve_scoped(policy.limits.max_materialized_bytes, "XML storage released")
                .expect("all temporary storage released"),
        );
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_shadowed_namespace_bounds_restore_at_element_depth() {
        let text = format!(
            "<r xmlns:p='urn:root'>{}{}<p:last p:x='1'/></r>",
            "<a xmlns:p='u'>".repeat(16),
            "</a>".repeat(16)
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 65_536;
        policy.limits.max_work_units = 64 * u64_from_index(text.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
        let tree = ctx
            .parse_xml(&text, "shadowed XML namespaces")
            .expect("shadowing keeps a local namespace count");
        let last = tree
            .document()
            .root_element()
            .last_element_child()
            .expect("p:last element");
        assert_eq!(last.tag_name().namespace(), Some("urn:root"));
        assert_eq!(last.attribute(("urn:root", "x")), Some("1"));
        drop(tree);
        drop(
            ctx.reserve_scoped(policy.limits.max_materialized_bytes, "XML storage released")
                .expect("all temporary storage released"),
        );
        ctx.finish_session().expect("unfused session");
    }

    #[test]
    fn tree_xml_unterminated_tag_keeps_attribute_work_bound() {
        let mut attributes = String::new();
        for index in 0..16 {
            std::fmt::Write::write_fmt(&mut attributes, format_args!(" x{index}='0'"))
                .expect("fixture string write");
        }
        let text = format!("<r{attributes}");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty input");
        let bound = super::xml_bound(&text, &ctx, "XML bound").expect("finite bound");
        let name_bytes = (0..16)
            .map(|index| format!("x{index}").len())
            .sum::<usize>();
        assert!(
            bound.local_comparisons.expect("finite bound")
                >= 17 * (u64_from_index(name_bytes) + 16)
        );
    }

    #[test]
    fn tree_xml_resource_dimensions() {
        for dimension in [
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
            ResourceDimension::RecursionDepth,
        ] {
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                _ => panic!("test dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
            let CodecError::ResourceLimit(limit) = ctx
                .parse_xml("<r/>", "XML tree")
                .expect_err("fixture must refuse")
            else {
                panic!("resource refusal");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }

    #[test]
    fn tree_xml_malformed_and_lexical_depth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        assert!(matches!(
            ctx.parse_xml("<r>", "XML tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
        ctx.parse_xml(
            "<r a='>'><!-- <a> --><![CDATA[<b>]]><?pi <c> ?></r>",
            "XML tree",
        )
        .expect("valid fixture");
        assert!(
            matches!(ctx.parse_xml("<r><s/></r>", "XML tree"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
        );
    }
    #[test]
    fn tree_malformed_diagnostic_preserves_retained_refusal() {
        for json in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) =
                DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
            let error = if json {
                ctx.parse_json_value("[", "JSON tree")
                    .expect_err("fixture must refuse")
            } else {
                ctx.parse_xml("<r>", "XML tree")
                    .expect_err("fixture must refuse")
            };
            let CodecError::ResourceLimit(limit) = error else {
                panic!("diagnostic admission must refuse");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }

    #[test]
    fn invalid_json_prefix_preserves_collection_refusal() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        let error = ctx
            .parse_json_value("[0,0,0,", "JSON nodes")
            .expect_err("fixture must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("invalid prefix must preserve its admission refusal");
        };
        assert_eq!(limit.operation, "JSON nodes");
        assert_eq!(Some(limit), ctx.resource_refusal());
    }

    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct JsonRecord {
        name: String,
        values: Vec<u64>,
    }

    #[test]
    fn tree_json_value_success_and_capacity_rounding() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        for text in ["[0]", "{}", "0", "\"a\""] {
            let (value, _scope) = ctx
                .parse_json_value(text, "JSON tree")
                .expect("valid fixture");
            assert_eq!(
                value,
                serde_json::from_str::<serde_json::Value>(text).expect("valid fixture")
            );
            let bound = ctx.json_bound(text, "JSON bound").expect("valid fixture");
            assert!(
                bound.bytes >= 4 * u64_from_index(std::mem::size_of::<serde_json::Value>()) + 1024
            );
        }
    }

    #[test]
    fn tree_json_typed_success() {
        let ctx = crate::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &ctx, &DecodePolicy::default())
            .expect("valid fixture");
        let record: JsonRecord = ctx
            .parse_json(r#"{"name":"r","values":[0]}"#, "typed JSON tree")
            .expect("valid fixture");
        assert_eq!(
            record,
            JsonRecord {
                name: "r".into(),
                values: vec![0]
            }
        );
    }

    #[test]
    fn tree_json_resource_dimensions() {
        for typed in [false, true] {
            for dimension in [
                ResourceDimension::CollectionItems,
                ResourceDimension::MaterializedBytes,
                ResourceDimension::WorkUnits,
                ResourceDimension::RecursionDepth,
                ResourceDimension::RetainedBytes,
            ] {
                if !typed && dimension == ResourceDimension::RetainedBytes {
                    continue;
                }
                let mut policy = DecodePolicy::default();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = 0;
                    }
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                    ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
                    ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                    _ => panic!("test dimension"),
                }
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
                let error = if typed {
                    ctx.parse_json::<Vec<u64>>("[0]", "typed JSON tree")
                        .expect_err("fixture must refuse")
                } else {
                    ctx.parse_json_value("[0]", "JSON tree")
                        .expect_err("fixture must refuse")
                };
                let CodecError::ResourceLimit(limit) = error else {
                    panic!("resource refusal");
                };
                assert_eq!(limit.dimension, dimension);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        }
    }

    #[test]
    fn tree_json_malformed_preserves_parse_route() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        assert!(matches!(
            ctx.parse_json_value("[", "JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.parse_json::<JsonRecord>("[", "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.parse_json::<JsonRecord>("{}", "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
    }

    #[test]
    fn tree_json_typed_preserves_ignored_raw_carrier_extensions() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let text =
            r#"{"name":"r","values":[],"extension":{"$serde_json::private::RawValue":"not-json"}}"#;
        let record: JsonRecord = ctx
            .parse_json(text, "typed JSON tree")
            .expect("valid fixture");
        assert_eq!(
            record,
            JsonRecord {
                name: "r".into(),
                values: Vec::new()
            }
        );
    }

    #[test]
    fn tree_json_typed_preserves_duplicate_field_errors() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let text = r#"{"name":"a","name":"b","values":[]}"#;
        assert!(matches!(
            ctx.parse_json::<JsonRecord>(text, "typed JSON tree"),
            Err(CodecError::Malformed(_))
        ));
        assert!(ctx.resource_refusal().is_none());
        let (value, _scope) = ctx
            .parse_json_value(text, "JSON tree")
            .expect("valid fixture");
        assert_eq!(value["name"], "b");
    }

    #[test]
    fn tree_json_lexical_and_raw_carrier_depth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("valid fixture");
        ctx.parse_json_value(r#"["[[["]"#, "JSON tree")
            .expect("valid fixture");
        let text = r#"{"$serde_json::private::RawValue":"\u005b0\u005d"}"#;
        assert!(
            matches!(ctx.parse_json_value(text, "JSON tree"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RecursionDepth)
        );
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::default())
            .expect("valid fixture");
        let (value, _scope) = ctx
            .parse_json_value(text, "JSON tree")
            .expect("valid fixture");
        assert_eq!(value, serde_json::json!([0]));
        let escaped_key = r#"{"\u0024serde_json::private::RawValue":"[0]"}"#;
        let bound = ctx
            .json_bound(escaped_key, "JSON bound")
            .expect("valid fixture");
        assert!(bound.depth >= 2);
    }
}
