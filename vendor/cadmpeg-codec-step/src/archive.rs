// SPDX-License-Identifier: Apache-2.0
//! STEP Part 21 ZIP-container rules.

use cadmpeg_core::container::ContainerRole;

use std::fmt::Write;
use std::path::Path;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) mod recovery;

/// The required root member name from Part 21 Annex A.4.
pub(crate) const ROOT_NAME: &str = "ISO-10303.p21";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReferenceTarget<'a> {
    Internal {
        member: String,
        query: Option<&'a str>,
        fragment: Option<&'a str>,
    },
    External,
}

/// Returns whether the input begins with a ZIP local-file header.
pub(crate) fn has_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Returns whether a detection prefix names the required STEP root member.
pub(crate) fn has_root_marker(
    ctx: &DecodeContext<'_>,
    prefix: View<'_>,
) -> Result<bool, CodecError> {
    // The root name is evidence only in the structured central directory.
    ctx.charge_work(
        u64_from_index(prefix.window().len().min(4)),
        "STEP ZIP detection magic",
    )?;
    if !has_zip_magic(prefix.window()) && !ArchiveSnapshot::has_footer(ctx, prefix.window())? {
        return Ok(false);
    }
    match ArchiveSnapshot::contains_matching_name(ctx, prefix, |name| name == ROOT_NAME) {
        Ok(found) => Ok(found),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(false),
    }
}

/// One STEP ZIP container whose required root member is proven present.
pub(crate) struct OpenedRoot<'a> {
    pub(crate) archive: ArchiveSnapshot<'a>,
    pub(crate) view: View<'a>,
    pub(crate) data_start: u64,
}

/// Opens and validates the required root member of one STEP ZIP container.
pub(crate) fn open_root<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<OpenedRoot<'a>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
    }
    let root_entry = archive.entry(ROOT_NAME).ok_or_else(|| {
        CodecError::WrongFormat(format!("STEP ZIP has no required root {ROOT_NAME}"))
    })?;
    let data_start = root_entry
        .data_start
        .ok_or_else(|| CodecError::malformed("STEP ZIP root has no readable local frame"))?;
    let root_view = archive.open(ctx, &root_entry.name)?;
    Ok(OpenedRoot {
        archive,
        view: root_view,
        data_start,
    })
}

/// Resolves one archive URI against the directory of its referencing member.
fn resolve_uri<'a>(
    ctx: &DecodeContext<'_>,
    member_bytes: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    base_member: &'a str,
    uri: &'a str,
) -> Result<ReferenceTarget<'a>, CodecError> {
    if has_uri_scheme(uri) || uri.starts_with("//") {
        return Ok(ReferenceTarget::External);
    }
    let (uri, fragment) = uri
        .split_once('#')
        .map_or((uri, None), |(uri, fragment)| (uri, Some(fragment)));
    if fragment.is_some_and(|fragment| fragment.contains('#')) {
        return Err(CodecError::malformed(format_args!(
            "invalid STEP ZIP URI fragment {uri:?}"
        )));
    }
    let (path, query) = uri
        .split_once('?')
        .map_or((uri, None), |(path, query)| (path, Some(query)));
    if path.starts_with('/') {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI escapes the archive root: {uri:?}"
        )));
    }
    let mut components = Vec::new();
    let mut component_bytes = ctx.reserve_scoped(0, "step_zip_uri_components_temp")?;
    if let Some((directory, _)) = base_member.rsplit_once('/') {
        for component in directory.split('/') {
            ctx.push_scoped_vec(
                &mut component_bytes,
                &mut components,
                component,
                "step_zip_uri_components",
            )?;
        }
    }
    if path.is_empty() {
        let member = ctx.copy_scoped_text(base_member, member_bytes, "step_zip_uri_member")?;
        return Ok(ReferenceTarget::Internal {
            member,
            query,
            fragment,
        });
    }
    for component in path.split('/') {
        match component {
            "" => {
                return Err(CodecError::malformed(format_args!(
                    "invalid empty path component in STEP ZIP URI {uri:?}"
                )));
            }
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(CodecError::malformed(format_args!(
                        "STEP ZIP URI escapes the archive root: {uri:?}"
                    )));
                }
            }
            component => ctx.push_scoped_vec(
                &mut component_bytes,
                &mut components,
                component,
                "step_zip_uri_components",
            )?,
        }
    }
    if components.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "STEP ZIP URI resolves to no member: {uri:?}"
        )));
    }
    let member_len = components
        .iter()
        .try_fold(0_usize, |total, component| {
            total.checked_add(component.len())
        })
        .and_then(|total| total.checked_add(components.len() - 1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_uri_member", 0, 1))?;
    let mut member = String::new();

    ctx.reserve_scoped_string(member_bytes, &mut member, member_len, "step_zip_uri_member")?;
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            member.push('/');
        }
        member.push_str(component);
    }
    Ok(ReferenceTarget::Internal {
        member,
        query,
        fragment,
    })
}

/// Resolves all root-file resource bindings and checks internal members.
pub(crate) fn root_reference_notes(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    exchange: &crate::parse::Exchange,
) -> Result<Vec<String>, CodecError> {
    // CE-02: Annex A.4 makes subsidiary access a root reference operation;
    // this pass records the binding and does not import a subsidiary graph.
    let mut notes = Vec::new();
    for reference in exchange.references() {
        let name = reference.name;
        let uri = forwarded_reference_uri(exchange, &reference.uri, ctx)?;
        let mut member_bytes = ctx.reserve_scoped(0, "step_zip_uri_member_temp")?;
        match resolve_uri(ctx, &mut member_bytes, ROOT_NAME, uri)? {
            ReferenceTarget::Internal {
                member,
                query,
                fragment,
            } => {
                if archive.entry(&member).is_none() {
                    return Err(CodecError::malformed(format_args!(
                        "STEP ZIP resource {uri:?} for {name} has no archive member {member:?}"
                    )));
                }
                push_reference_note(
                    ctx,
                    &mut notes,
                    "internal resource ",
                    name,
                    &member,
                    query,
                    fragment,
                )?;
            }
            ReferenceTarget::External => {
                push_reference_note(ctx, &mut notes, "external resource ", name, uri, None, None)?;
            }
        }
    }
    Ok(notes)
}

fn push_reference_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    prefix: &str,
    name: crate::parse::ReferenceName,
    target: &str,
    query: Option<&str>,
    fragment: Option<&str>,
) -> Result<(), CodecError> {
    let (marker, mut id) = match name {
        crate::parse::ReferenceName::Entity(id) => ('#', id),
        crate::parse::ReferenceName::Value(id) => ('@', id),
    };
    let mut digits = 1_usize;
    while id >= 10 {
        id /= 10;
        digits += 1;
    }
    let query_len = query
        .map_or(Some(0), |text| text.len().checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    let fragment_len = fragment
        .map_or(Some(0), |text| text.len().checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    let len = prefix
        .len()
        .checked_add(1 + digits + " -> ".len())
        .and_then(|len| len.checked_add(target.len()))
        .and_then(|len| len.checked_add(query_len))
        .and_then(|len| len.checked_add(fragment_len))
        .ok_or_else(|| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    ctx.reserve_vec(notes, 1, "step_zip_reference_notes")?;

    let mut note = ctx.retained_string(len, "step_zip_reference_note")?;

    note.push_str(prefix);
    note.push(marker);
    let id = match name {
        crate::parse::ReferenceName::Entity(id) | crate::parse::ReferenceName::Value(id) => id,
    };
    write!(&mut note, "{id}")
        .map_err(|_| ctx.refuse_codec_limit("step_zip_reference_note", 0, 1))?;
    note.push_str(" -> ");
    note.push_str(target);
    if let Some(query) = query {
        note.push('?');
        note.push_str(query);
    }
    if let Some(fragment) = fragment {
        note.push('#');
        note.push_str(fragment);
    }
    notes.push(note);
    Ok(())
}

fn forwarded_reference_uri<'a>(
    exchange: &'a crate::parse::Exchange,
    uri: &'a str,
    ctx: &DecodeContext<'_>,
) -> Result<&'a str, CodecError> {
    let Some(fragment) = uri.strip_prefix('#') else {
        return Ok(uri);
    };
    for anchor in exchange.anchors() {
        ctx.charge_work(1, "step_zip_anchor_lookup")?;
        if anchor.name.len() != fragment.len() {
            continue;
        }
        ctx.charge_work(u64_from_index(fragment.len()), "step_zip_anchor_lookup")?;
        if anchor.name == fragment {
            return Ok(match &anchor.value {
                crate::parse::Value::Resource(target) => target.as_str(),
                _ => uri,
            });
        }
    }
    Ok(uri)
}

fn has_uri_scheme(uri: &str) -> bool {
    let Some(colon) = uri.find(':') else {
        return false;
    };
    let scheme = &uri[..colon];
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Classifies a physical ZIP member for the STEP container report.
pub(crate) fn classify_entry(name: &str) -> ContainerRole {
    match name {
        ROOT_NAME => ContainerRole::RootExchange,
        _ if name.ends_with('/') => ContainerRole::Directory,
        _ if extension_is(name, "p21")
            || extension_is(name, "step")
            || extension_is(name, "stp") =>
        {
            ContainerRole::SubsidiaryExchange
        }
        _ if extension_is(name, "zip") => ContainerRole::NestedArchive,
        _ => ContainerRole::Ancillary,
    }
}

fn extension_is(name: &str, expected: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn validate_entry_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.split('/').enumerate().any(|(index, component)| {
            component.is_empty() && !(index == name.split('/').count() - 1 && name.ends_with('/'))
                || component == "."
                || component == ".."
        })
    {
        return Err(CodecError::malformed(format_args!(
            "unsafe STEP ZIP entry path {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
