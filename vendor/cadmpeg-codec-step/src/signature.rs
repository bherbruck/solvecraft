// SPDX-License-Identifier: Apache-2.0
//! Structural validation for detached CMS signatures in Part 21.

use std::ops::Range;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};

use crate::parse::ParseError;

pub(crate) fn decode_payload(
    input: &[u8],
    payload: &Range<usize>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u8>, ParseError> {
    let mut compact = Vec::new();
    let mut compact_reservation = ctx.reserve_scoped(0, "step_signature_compact_temp")?;
    let mut at = payload.start;
    while at < payload.end {
        if input[at].is_ascii_control() || input[at] == b' ' {
            at += 1;
            continue;
        }
        if let Some(end) = crate::lex::print_control_end(input, at) {
            if end <= payload.end {
                at = end;
                continue;
            }
        }
        if input.get(at..at + 2) == Some(b"/*") {
            let body = at + 2;
            if let Some(end) = input[body..payload.end]
                .windows(2)
                .position(|window| window == b"*/")
            {
                at = body + end + 2;
                continue;
            }
        }

        ctx.reserve_scoped_vec(
            &mut compact_reservation,
            &mut compact,
            1,
            "step_signature_compact_items",
        )?;
        compact.push(input[at]);
        at += 1;
    }
    let estimate = base64::decoded_len_estimate(compact.len());
    let _cms_reservation =
        ctx.reserve_scoped(u64_from_index(estimate), "step_signature_cms_temp")?;
    let mut cms = ctx.alloc_filled(estimate, 0_u8, "step_signature_cms_bytes")?;
    let decoded =
        STANDARD
            .decode_slice(&compact, &mut cms)
            .map_err(|error| ParseError::Syntax {
                offset: payload.start,
                message: format!("invalid SIGNATURE Base64 payload: {error}"),
            })?;
    cms.truncate(decoded);
    // SG-04: this is a structural detached-CMS gate. It does not compute the
    // Part 21 alphabet digest, verify a signer key, or apply caller policy;
    // the codec retains an admitted signature as opaque source data.
    validate_detached_cms(&cms).map_err(|message| ParseError::Syntax {
        offset: payload.start,
        message: format!("invalid detached CMS SIGNATURE payload: {message}"),
    })?;
    Ok(cms)
}

const CMS_SIGNED_DATA_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];

#[derive(Debug)]
struct Ber<'a> {
    input: &'a [u8],
    at: usize,
}

impl<'a> Ber<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, at: 0 }
    }

    fn remaining(&self) -> Result<usize, &'static str> {
        self.input
            .len()
            .checked_sub(self.at)
            .ok_or("BER cursor exceeds input")
    }

    fn take(&mut self) -> Result<(u8, &'a [u8]), &'static str> {
        let tag = self.take_tag_octet()?;
        let first_length = *self.input.get(self.at).ok_or("missing BER length")?;
        self.at += 1;
        if first_length == 0x80 {
            if tag & 0x20 == 0 {
                return Err("indefinite length on primitive CMS value");
            }
            let value_start = self.at;
            let value_end = self.indefinite_end(value_start)?;
            self.at = value_end
                .checked_add(2)
                .ok_or("BER end-of-contents overflow")?;
            return Ok((tag, &self.input[value_start..value_end]));
        }
        let length = if first_length & 0x80 == 0 {
            usize::from(first_length)
        } else {
            let octets = usize::from(first_length & 0x7f);
            if octets == 0 || octets > std::mem::size_of::<usize>() {
                return Err("unsupported BER length");
            }
            let end = self.at.checked_add(octets).ok_or("BER length overflow")?;
            let bytes = self.input.get(self.at..end).ok_or("truncated BER length")?;
            self.at = end;
            bytes.iter().try_fold(0usize, |value, byte| {
                value
                    .checked_shl(8)
                    .and_then(|value| value.checked_add(usize::from(*byte)))
                    .ok_or("BER length overflow")
            })?
        };
        let end = self.at.checked_add(length).ok_or("BER value overflow")?;
        let value = self.input.get(self.at..end).ok_or("truncated BER value")?;
        self.at = end;
        Ok((tag, value))
    }

    fn take_tag_octet(&mut self) -> Result<u8, &'static str> {
        let tag = *self.input.get(self.at).ok_or("missing BER tag")?;
        self.at += 1;
        if tag & 0x1f == 0x1f {
            let mut octets = 0;
            loop {
                let byte = *self.input.get(self.at).ok_or("truncated BER tag")?;
                self.at += 1;
                octets += 1;
                if octets > std::mem::size_of::<usize>() * 8 {
                    return Err("BER tag is too long");
                }
                if byte & 0x80 == 0 {
                    break;
                }
            }
        }
        Ok(tag)
    }

    fn indefinite_end(&self, start: usize) -> Result<usize, &'static str> {
        let mut contents = Self {
            input: self.input,
            at: start,
        };
        loop {
            if contents
                .input
                .get(contents.at..)
                .is_some_and(|remaining| remaining.starts_with(&[0, 0]))
            {
                return Ok(contents.at);
            }
            if contents.at >= contents.input.len() {
                return Err("unterminated BER indefinite value");
            }
            contents.take()?;
        }
    }

    fn take_tag(&mut self, expected: u8) -> Result<&'a [u8], &'static str> {
        let (tag, value) = self.take()?;
        (tag == expected)
            .then_some(value)
            .ok_or("unexpected BER tag")
    }
}

fn validate_integer(value: &[u8]) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("empty CMS integer");
    }
    if value.len() > 1
        && ((value[0] == 0 && value[1] & 0x80 == 0) || (value[0] == 0xff && value[1] & 0x80 != 0))
    {
        return Err("non-minimal CMS integer");
    }
    Ok(())
}

fn validate_algorithm_identifier(value: &[u8]) -> Result<(), &'static str> {
    let mut algorithm = Ber::new(value);
    if algorithm.take_tag(0x06)?.is_empty() {
        return Err("empty CMS algorithm OID");
    }
    while algorithm.remaining()? > 0 {
        algorithm.take()?;
        if algorithm.remaining()? > 0 {
            return Err("CMS algorithm identifier has multiple parameters");
        }
    }
    Ok(())
}

fn validate_octet_string(tag: u8, value: &[u8]) -> Result<(), &'static str> {
    match tag {
        0x04 => Ok(()),
        0x24 => {
            let mut chunks = Ber::new(value);
            while chunks.remaining()? > 0 {
                let (chunk_tag, chunk_value) = chunks.take()?;
                validate_octet_string(chunk_tag, chunk_value)?;
            }
            Ok(())
        }
        _ => Err("invalid CMS OCTET STRING"),
    }
}

fn validate_subject_key_identifier(tag: u8, value: &[u8]) -> Result<(), &'static str> {
    match tag {
        0x80 => Ok(()),
        0xa0 => {
            let mut chunks = Ber::new(value);
            while chunks.remaining()? > 0 {
                let (chunk_tag, chunk_value) = chunks.take()?;
                validate_octet_string(chunk_tag, chunk_value)?;
            }
            Ok(())
        }
        _ => Err("invalid CMS subject key identifier"),
    }
}

fn validate_digest_algorithms(value: &[u8]) -> Result<(), &'static str> {
    let mut algorithms = Ber::new(value);
    if algorithms.remaining()? == 0 {
        return Err("CMS SignedData has no digest algorithm");
    }
    while algorithms.remaining()? > 0 {
        let algorithm = algorithms.take_tag(0x30)?;
        validate_algorithm_identifier(algorithm)?;
    }
    Ok(())
}

fn validate_signer_identifier(tag: u8, value: &[u8]) -> Result<(), &'static str> {
    match tag {
        0x30 => {
            let mut issuer_and_serial = Ber::new(value);
            let issuer = issuer_and_serial.take_tag(0x30)?;
            let mut issuer = Ber::new(issuer);
            while issuer.remaining()? > 0 {
                issuer.take()?;
            }
            validate_integer(issuer_and_serial.take_tag(0x02)?)?;
            require_empty(&issuer_and_serial)
        }
        0x80 | 0xa0 => validate_subject_key_identifier(tag, value),
        _ => Err("invalid CMS signer identifier"),
    }
}

fn validate_signer_info(value: &[u8]) -> Result<(), &'static str> {
    let mut signer = Ber::new(value);
    validate_integer(signer.take_tag(0x02)?)?;
    let (signer_identifier_tag, signer_identifier) = signer.take()?;
    validate_signer_identifier(signer_identifier_tag, signer_identifier)?;
    validate_algorithm_identifier(signer.take_tag(0x30)?)?;
    if signer.input.get(signer.at).copied() == Some(0xa0) {
        signer.take()?;
    }
    validate_algorithm_identifier(signer.take_tag(0x30)?)?;
    let (signature_tag, signature_value) = signer.take()?;
    validate_octet_string(signature_tag, signature_value)?;
    if signer.input.get(signer.at).copied() == Some(0xa1) {
        signer.take()?;
    }
    require_empty(&signer)
}

fn validate_signer_infos(value: &[u8]) -> Result<(), &'static str> {
    let mut signers = Ber::new(value);
    if signers.remaining()? == 0 {
        return Err("CMS SignedData has no signer");
    }
    while signers.remaining()? > 0 {
        validate_signer_info(signers.take_tag(0x30)?)?;
    }
    Ok(())
}

fn require_empty(ber: &Ber<'_>) -> Result<(), &'static str> {
    (ber.remaining()? == 0)
        .then_some(())
        .ok_or("trailing BER value")
}

/// Checks the Part 21 CMS envelope and the detached-content invariant.
///
/// This admits structure only. It does not compute a content digest, verify a
/// signature value, select a public key, or apply a caller trust policy.
fn validate_detached_cms(input: &[u8]) -> Result<(), &'static str> {
    let mut content_info = Ber::new(input);
    let content_info_value = content_info.take_tag(0x30)?;
    require_empty(&content_info)?;

    let mut content_info = Ber::new(content_info_value);
    let content_type = content_info.take_tag(0x06)?;
    if content_type != CMS_SIGNED_DATA_OID {
        return Err("CMS content type is not signedData");
    }
    let signed_data_wrapper = content_info.take_tag(0xa0)?;
    require_empty(&content_info)?;

    let mut wrapper = Ber::new(signed_data_wrapper);
    let signed_data_value = wrapper.take_tag(0x30)?;
    require_empty(&wrapper)?;

    let mut signed_data = Ber::new(signed_data_value);
    validate_integer(signed_data.take_tag(0x02)?)?;
    validate_digest_algorithms(signed_data.take_tag(0x31)?)?;
    let encap_content_info = signed_data.take_tag(0x30)?;
    let mut encap_content_info = Ber::new(encap_content_info);
    encap_content_info.take_tag(0x06)?;
    if encap_content_info.remaining()? != 0 {
        return Err("CMS SignedData is not detached");
    }

    let mut optional_stage = 0;
    while signed_data.remaining()? > 0 {
        let (tag, value) = signed_data.take()?;
        match tag {
            0xa0 | 0xa1 => {
                let stage = if tag == 0xa0 { 1 } else { 2 };
                if stage <= optional_stage {
                    return Err("CMS optional fields are out of order");
                }
                optional_stage = stage;
                let mut optional = Ber::new(value);
                while optional.remaining()? > 0 {
                    optional.take()?;
                }
            }
            0x31 => {
                validate_signer_infos(value)?;
                require_empty(&signed_data)?;
                return Ok(());
            }
            _ => return Err("unexpected CMS SignedData field"),
        }
    }
    Err("CMS SignedData has no signer set")
}

#[cfg(test)]
mod tests;
