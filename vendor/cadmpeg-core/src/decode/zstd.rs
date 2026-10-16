// SPDX-License-Identifier: Apache-2.0
//! Window-admitted Zstandard streams with scoped decoder workspace.

use super::{
    u64_from_index, DecodeContext, ResourceDimension, ResourceFailure, ScopedReservation, View,
};
use crate::CodecError;
use zstd::zstd_safe::{self, DCtx, DParameter, InBuffer, OutBuffer};

const BLOCK_BYTES: u64 = 128 * 1024;
const FIXED_STATE_BYTES: u64 = 1024 * 1024;
const COPY_PADDING_BYTES: u64 = 64;
const MAX_DECODER_WINDOW: u64 = 1_u64 << if usize::BITS == 32 { 30 } else { 31 };

/// One decoder and its live workspace reservation. The decoder is dropped
/// before the reservation that admits its allocation.
pub struct ZstdDecoder<'ctx, 'input> {
    decoder: DCtx<'static>,
    _workspace: ScopedReservation<'ctx>,
    ctx: &'ctx DecodeContext<'input>,
    input: InBuffer<'input>,
    finished: bool,
    workspace_bytes: u64,
    window_bytes: u64,
}

impl<'input> DecodeContext<'input> {
    /// Inspects all frame windows before allocating a decoder. The largest
    /// frame window must fit the materialized-byte and per-expansion ceilings.
    pub fn open_zstd<'ctx>(
        &'ctx self,
        source: View<'input>,
    ) -> Result<ZstdDecoder<'ctx, 'input>, CodecError> {
        self.charge_work(
            u64_from_index(source.window().len()),
            "inspect Zstandard frames",
        )?;
        self.charge_work(
            u64_from_index(source.window().len()),
            "Zstandard compressed input",
        )?;
        let mut offset = 0;
        let mut largest_window = 0_u64;
        let ceiling = self
            .policy()
            .limits
            .max_materialized_bytes
            .min(self.policy().limits.max_decompressed_bytes_per_expand)
            .min(MAX_DECODER_WINDOW);
        while offset < source.window().len() {
            self.charge_work(1, "inspect Zstandard frame header")?;
            let frame = source
                .child(source.start() + offset, source.end())
                .ok_or_else(|| {
                    CodecError::Malformed("Zstandard frame offset escapes input".into())
                })?;
            let window = frame_window(frame)?;
            if window > ceiling {
                return Err(self.budget.refuse(
                    ResourceDimension::MaterializedBytes,
                    ResourceFailure::BudgetExceeded,
                    ceiling,
                    0,
                    window,
                    "Zstandard window",
                ));
            }
            largest_window = largest_window.max(window);
            let size = zstd_safe::find_frame_compressed_size(frame.window())
                .map_err(|code| zstd_error(self, code, 0, window))?;
            if size == 0 || size > frame.window().len() {
                return Err(CodecError::Malformed("invalid Zstandard frame size".into()));
            }
            offset += size;
        }
        if offset == 0 {
            return Err(CodecError::Malformed("empty Zstandard stream".into()));
        }
        // ZSTD_estimateDStreamSize in zstd 1.5.7 bounds streaming storage by
        // DCtx state + window + three 128 KiB blocks + 2*WILDCOPY_OVERLENGTH.
        // The fixed state, including entropy tables and the literal scratch
        // buffer, is below 1 MiB. Dictionaries and legacy frames are not opened.
        let workspace_bytes = largest_window
            .checked_add(3 * BLOCK_BYTES + FIXED_STATE_BYTES + COPY_PADDING_BYTES)
            .ok_or_else(|| {
                CodecError::from(
                    self.budget
                        .scoped_size_overflow_limit("Zstandard workspace"),
                )
            })?;
        let workspace = self.reserve_scoped(workspace_bytes, "Zstandard workspace")?;
        let mut decoder = DCtx::try_create().ok_or_else(|| {
            self.budget
                .scoped_allocation_failed(workspace_bytes, "Zstandard context")
        })?;
        // Round the inspected maximum up to the next permitted log, with the
        // decoder's format minimum of 1 KiB. Each actual frame was checked
        // against the exact ceiling above before allocation.
        let minimum_window = largest_window.max(1024);
        let window_log = u64::BITS - (minimum_window - 1).leading_zeros();
        decoder
            .set_parameter(DParameter::WindowLogMax(window_log))
            .map_err(|code| zstd_error(self, code, workspace_bytes, largest_window))?;
        Ok(ZstdDecoder {
            decoder,
            _workspace: workspace,
            ctx: self,
            input: InBuffer {
                src: source.window(),
                pos: 0,
            },
            finished: false,
            workspace_bytes,
            window_bytes: largest_window,
        })
    }
}

impl ZstdDecoder<'_, '_> {
    /// Reads admitted output into caller-owned storage and charges step/copy work.
    pub fn read_chunk(&mut self, bytes: &mut [u8]) -> Result<usize, CodecError> {
        if self.finished || bytes.is_empty() {
            return Ok(0);
        }
        loop {
            self.ctx
                .charge_work(u64_from_index(bytes.len()), "Zstandard decode step")?;
            let before = self.input.pos;
            let mut chunk = [0_u8; 16 * 1024];
            let capacity = bytes.len().min(chunk.len());
            let mut output = OutBuffer::around(&mut chunk[..capacity]);
            let remaining = self
                .decoder
                .decompress_stream(&mut output, &mut self.input)
                .map_err(|code| {
                    zstd_error(self.ctx, code, self.workspace_bytes, self.window_bytes)
                })?;
            let produced = output.pos();
            self.ctx
                .charge_work(u64_from_index(produced), "Zstandard output copy")?;
            bytes[..produced].copy_from_slice(&chunk[..produced]);
            if remaining == 0 && self.input.pos == self.input.src.len() {
                self.finished = true;
            }
            if produced != 0 || self.finished {
                return Ok(produced);
            }
            if before == self.input.pos {
                return Err(CodecError::Malformed("truncated Zstandard stream".into()));
            }
        }
    }
}

fn zstd_error(ctx: &DecodeContext<'_>, code: usize, bytes: u64, window: u64) -> CodecError {
    // SAFETY: the code is the error result returned by the same zstd library.
    let error = unsafe { zstd_safe::zstd_sys::ZSTD_getErrorCode(code) };
    match error {
        zstd_safe::zstd_sys::ZSTD_ErrorCode::ZSTD_error_memory_allocation => ctx
            .budget
            .scoped_allocation_failed(bytes, "Zstandard decoder allocation"),
        zstd_safe::zstd_sys::ZSTD_ErrorCode::ZSTD_error_frameParameter_windowTooLarge => {
            ctx.budget.refuse(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::BudgetExceeded,
                ctx.policy()
                    .limits
                    .max_materialized_bytes
                    .min(ctx.policy().limits.max_decompressed_bytes_per_expand)
                    .min(MAX_DECODER_WINDOW),
                0,
                window,
                "Zstandard decoder window",
            )
        }
        _ => CodecError::malformed(format_args!(
            "invalid Zstandard stream: {}",
            zstd_safe::get_error_name(code)
        )),
    }
}

fn frame_window(mut frame: View<'_>) -> Result<u64, CodecError> {
    let magic = frame.req_u32_le()?;
    if (0x184d_2a50..=0x184d_2a5f).contains(&magic) {
        frame.req_u32_le()?;
        return Ok(0);
    }
    if magic != 0xfd2f_b528 {
        return Err(CodecError::Malformed("invalid Zstandard magic".into()));
    }
    let flags = frame.req_u8()?;
    if flags & 8 != 0 {
        return Err(CodecError::Malformed(
            "reserved Zstandard frame flag".into(),
        ));
    }
    let single_segment = flags & 32 != 0;
    let window = if single_segment {
        0
    } else {
        let descriptor = frame.req_u8()?;
        let base = 1_u64 << (10 + u32::from(descriptor >> 3));
        base + (base / 8) * u64::from(descriptor & 7)
    };
    let dictionary_bytes = match flags & 3 {
        0 => 0,
        1 => 1,
        2 => 2,
        _ => 4,
    };
    frame.req_take(dictionary_bytes)?;
    let content_size = match flags >> 6 {
        0 if single_segment => u64::from(frame.req_u8()?),
        0 => 0,
        1 => u64::from(frame.req_u16_le()?) + 256,
        2 => u64::from(frame.req_u32_le()?),
        _ => frame.req_u64_le()?,
    };
    Ok(if single_segment { content_size } else { window })
}

#[cfg(test)]
mod tests {
    use super::{BLOCK_BYTES, COPY_PADDING_BYTES, FIXED_STATE_BYTES};
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn zstd_checks_each_window_before_decoder_allocation() {
        let mut bytes = zstd::bulk::compress(b"prefix", 0).expect("frame");
        bytes.extend_from_slice(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xa0, 1, 0, 0]);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        assert!(
            matches!(ctx.open_zstd(root), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "Zstandard window" && limit.additional == 1 << 30)
        );
    }

    #[test]
    fn zstd_workspace_stays_reserved_until_decoder_drop() {
        let bytes = zstd::bulk::compress(b"payload", 0).expect("frame");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            7 + 3 * BLOCK_BYTES + FIXED_STATE_BYTES + COPY_PADDING_BYTES;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let mut decoder = ctx.open_zstd(root).expect("decoder");
        assert_eq!(
            decoder.workspace_bytes,
            policy.limits.max_materialized_bytes
        );
        let mut output = [0; 7];
        assert_eq!(decoder.read_chunk(&mut output).expect("read"), 7);
        assert_eq!(&output, b"payload");
        drop(decoder);
        ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse workspace")
            .expect("released workspace");
    }

    #[test]
    fn zstd_workspace_and_work_limits_remain_typed() {
        let bytes = zstd::bulk::compress(b"payload", 0).expect("frame");
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::MaterializedBytes {
                policy.limits.max_materialized_bytes = 0;
            } else {
                policy.limits.max_work_units = 0;
            }
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            assert!(
                matches!(ctx.open_zstd(root), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
            );
        }
    }
}
