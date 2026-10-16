// SPDX-License-Identifier: Apache-2.0
//! ISO 10303-21 string escape decoding and canonical encoding.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::parse::implementation_level::ImplementationLevel;

/// A malformed or unsupported string escape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} at string byte {offset}")]
pub(crate) struct StringError {
    /// Byte position within the unquoted string token.
    offset: usize,
    /// Description of the violated escape invariant.
    message: String,
}

/// A malformed string or caller-context refusal while decoding it.
#[derive(Debug)]
pub(crate) enum StringDecodeFailure {
    Invalid(StringError),
    Resource(CodecError),
}

const MAX_STORED_STRING_OCTETS: usize = 32_767;

pub(crate) fn decode_with_context(
    input: &[u8],
    level: ImplementationLevel,
    ctx: &DecodeContext<'_>,
) -> Result<String, StringDecodeFailure> {
    if input.len() > MAX_STORED_STRING_OCTETS {
        return Err(StringDecodeFailure::Invalid(StringError {
            offset: 0,
            message: "string exceeds maximum stored length".to_owned(),
        }));
    }
    let len = decoded_len(input, level).map_err(StringDecodeFailure::Invalid)?;
    let operation = "step_string_text";
    let mut output = ctx
        .retained_string(len, operation)
        .map_err(StringDecodeFailure::Resource)?;

    decode_chars(input, level, |character| output.push(character))
        .map_err(StringDecodeFailure::Invalid)?;
    Ok(output)
}

fn decoded_len(input: &[u8], level: ImplementationLevel) -> Result<usize, StringError> {
    // One source byte expands to at most two UTF-8 bytes, so this sum fits usize.
    let mut len = 0usize;
    decode_chars(input, level, |character| len += character.len_utf8())?;
    Ok(len)
}

fn decode_chars(
    input: &[u8],
    level: ImplementationLevel,
    mut emit: impl FnMut(char),
) -> Result<(), StringError> {
    let mut at = 0;
    let mut page = b'A';
    while at < input.len() {
        match input[at] {
            b'\'' if input.get(at + 1) == Some(&b'\'') => {
                emit('\'');
                at += 2;
            }
            b'\\'
                if matches!(input.get(at + 1), Some(b'N' | b'F'))
                    && input.get(at + 2) == Some(&b'\\') =>
            {
                at += 3;
            }
            b'\\' if input.get(at + 1) == Some(&b'\\') => {
                emit('\\');
                at += 2;
            }
            b'\\' if input.get(at + 1) == Some(&b'P') => {
                if !matches!(input.get(at + 2), Some(b'A'..=b'I'))
                    || input.get(at + 3) != Some(&b'\\')
                {
                    return error(at, "invalid page-selection escape");
                }
                page = input[at + 2];
                at += 4;
            }
            b'\\' if input.get(at + 1) == Some(&b'S') => {
                if input.get(at + 2) != Some(&b'\\') {
                    return error(at, "invalid S escape");
                }
                let Some(&code) = input.get(at + 3) else {
                    return error(at, "truncated S escape");
                };
                emit(decode_page_byte(page, code | 0x80, at)?);
                at += 4;
            }
            b'\\' if input.get(at + 1) == Some(&b'X') => match input.get(at + 2) {
                Some(b'\\') => {
                    let byte = hex_byte(input, at + 3)?;
                    emit(char::from(byte));
                    at += 5;
                }
                Some(b'2') if input.get(at + 3) == Some(&b'\\') => {
                    at = decode_wide(input, at + 4, 4, &mut emit)?;
                }
                Some(b'4') if input.get(at + 3) == Some(&b'\\') => {
                    at = decode_wide(input, at + 4, 8, &mut emit)?;
                }
                _ => return error(at, "invalid X escape"),
            },
            b'\'' => return error(at, "unpaired apostrophe"),
            byte if byte.is_ascii_control() => at += 1,
            b'\\' => return error(at, "unknown reverse-solidus escape"),
            _ => {
                let start = at;
                while at < input.len() && !matches!(input[at], b'\'' | b'\\') {
                    at += 1;
                }
                let direct = &input[start..at];
                if level.is_edition3() {
                    let text = std::str::from_utf8(direct).map_err(|error| StringError {
                        offset: start + error.valid_up_to(),
                        message: "invalid UTF-8 direct string bytes".into(),
                    })?;
                    for character in text.chars() {
                        emit(character);
                    }
                } else {
                    for byte in direct {
                        emit(char::from(*byte));
                    }
                }
            }
        }
    }
    Ok(())
}

fn decode_page_byte(page: u8, byte: u8, offset: usize) -> Result<char, StringError> {
    let part = page - b'A' + 1;
    if byte < 0xa0 || part == 1 {
        return Ok(char::from(byte));
    }
    if part == 9 {
        return Ok(match byte {
            0xd0 => '\u{011e}',
            0xdd => '\u{0130}',
            0xde => '\u{015e}',
            0xf0 => '\u{011f}',
            0xfd => '\u{0131}',
            0xfe => '\u{015f}',
            _ => char::from(byte),
        });
    }
    let label = format!("iso-8859-{part}");
    let encoding =
        encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| StringError {
            offset,
            message: format!("ISO 8859 part {part} is unavailable"),
        })?;
    let bytes = [byte];
    let (decoded, had_errors) = encoding.decode_without_bom_handling(&bytes);
    if had_errors {
        return error(
            offset,
            "S escape is undefined in the selected ISO 8859 part",
        );
    }
    decoded.chars().next().ok_or_else(|| StringError {
        offset,
        message: "S escape decoded to no character".into(),
    })
}

/// Encode text as bytes suitable between Part 21 apostrophe delimiters.
pub(crate) fn encode(input: &str) -> String {
    let mut output = String::new();
    for character in input.chars() {
        match character {
            '\'' => output.push_str("''"),
            '\\' => output.push_str("\\\\"),
            '\u{20}'..='\u{7e}' => output.push(character),
            character if u32::from(character) <= 0xffff => {
                output.push_str("\\X2\\");
                push_hex_digits(&mut output, &u32::from(character).to_be_bytes()[2..]);
                output.push_str("\\X0\\");
            }
            character => {
                output.push_str("\\X4\\");
                push_hex_digits(&mut output, &u32::from(character).to_be_bytes());
                output.push_str("\\X0\\");
            }
        }
    }
    output
}

fn push_hex_digits(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0xF)]));
    }
}

fn decode_wide(
    input: &[u8],
    start: usize,
    width: usize,
    emit: &mut impl FnMut(char),
) -> Result<usize, StringError> {
    let Some(relative_end) = input[start..]
        .windows(4)
        .position(|bytes| bytes == b"\\X0\\")
    else {
        return error(start, "unterminated wide escape");
    };
    let end = start + relative_end;
    if !(end - start).is_multiple_of(width) {
        return error(start, "wide escape has incomplete code unit");
    }
    let mut high_surrogate = None;
    for offset in (start..end).step_by(width) {
        let raw = std::str::from_utf8(&input[offset..offset + width])
            .ok()
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .ok_or_else(|| StringError {
                offset,
                message: "wide escape contains non-hexadecimal digits".into(),
            })?;
        if width == 4 {
            let Ok(unit) = u16::try_from(raw) else {
                return error(offset, "wide escape contains non-hexadecimal digits");
            };
            match unit {
                0xd800..=0xdbff => {
                    if high_surrogate.replace(unit).is_some() {
                        return error(start, "wide escape contains an isolated surrogate");
                    }
                }
                0xdc00..=0xdfff => {
                    let Some(high) = high_surrogate.take() else {
                        return error(start, "wide escape contains an isolated surrogate");
                    };
                    let scalar =
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(unit) - 0xdc00);
                    let Some(character) = char::from_u32(scalar) else {
                        return error(start, "wide escape contains an isolated surrogate");
                    };
                    emit(character);
                }
                _ => {
                    if high_surrogate.is_some() {
                        return error(start, "wide escape contains an isolated surrogate");
                    }
                    let Some(character) = char::from_u32(u32::from(unit)) else {
                        return error(start, "wide escape contains an isolated surrogate");
                    };
                    emit(character);
                }
            }
        } else {
            let character = char::from_u32(raw).ok_or_else(|| StringError {
                offset: start,
                message: "wide escape contains an invalid Unicode scalar".into(),
            })?;
            emit(character);
        }
    }
    if high_surrogate.is_some() {
        return error(start, "wide escape contains an isolated surrogate");
    }
    Ok(end + 4)
}

fn hex_byte(input: &[u8], offset: usize) -> Result<u8, StringError> {
    let Some(bytes) = input.get(offset..offset + 2) else {
        return error(offset, "truncated byte escape");
    };
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .ok_or_else(|| StringError {
            offset,
            message: "byte escape contains non-hexadecimal digits".into(),
        })
}

fn error<T>(offset: usize, message: &str) -> Result<T, StringError> {
    Err(StringError {
        offset,
        message: message.into(),
    })
}

#[cfg(test)]
pub(crate) mod tests;
