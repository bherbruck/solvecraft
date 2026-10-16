// SPDX-License-Identifier: Apache-2.0
//! Scoped DEFLATE output for detection.

use super::{u64_from_index, DecodeContext, ScopedReservation, View};
use crate::CodecError;
use flate2::{Decompress, FlushDecompress, Status};

impl DecodeContext<'_> {
    /// Inflates one raw or zlib member up to `cap`. An oversized or malformed
    /// member supplies no evidence. Resource refusals remain typed.
    pub fn inflate_probe(
        &self,
        source: View<'_>,
        cap: usize,
        zlib: bool,
    ) -> Result<Option<(Vec<u8>, ScopedReservation<'_>)>, CodecError> {
        self.charge_work(
            u64_from_index(source.window().len()),
            "probe compressed input",
        )?;
        // miniz_oxide uses a 32 KiB dictionary and fixed Huffman tables. This
        // 256 KiB bound includes the state and table storage for one decoder.
        let _workspace = self.reserve_scoped(256 * 1024, "DEFLATE probe workspace")?;
        let mut decoder = Decompress::new(zlib);
        let mut storage = self.reserve_scoped(0, "DEFLATE probe output")?;
        let mut output = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut offset = 0;
        loop {
            self.charge_work(u64_from_index(chunk.len()), "DEFLATE probe step")?;
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let Ok(status) = decoder.decompress(
                &source.window()[offset..],
                &mut chunk,
                FlushDecompress::None,
            ) else {
                return Ok(None);
            };
            let consumed = usize::try_from(decoder.total_in() - before_in)
                .map_err(|_| CodecError::Malformed("DEFLATE probe input overflow".into()))?;
            let produced = usize::try_from(decoder.total_out() - before_out)
                .map_err(|_| CodecError::Malformed("DEFLATE probe output overflow".into()))?;
            offset += consumed;
            if cap
                .checked_sub(output.len())
                .is_none_or(|remaining| produced > remaining)
            {
                return Ok(None);
            }
            self.charge_work(u64_from_index(produced), "DEFLATE probe copy")?;
            self.reserve_scoped_vec(&mut storage, &mut output, produced, "DEFLATE probe output")?;
            output.extend_from_slice(&chunk[..produced]);
            if status == Status::StreamEnd {
                return Ok(Some((output, storage)));
            }
            if consumed == 0 && produced == 0 {
                return Ok(Some((output, storage)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;
    use flate2::{write::DeflateEncoder, Compression};
    use std::io::Write;

    #[test]
    fn deflate_probe_output_reservation_lives_with_returned_bytes() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"Document.xml").expect("encode");
        let bytes = encoder.finish().expect("finish");
        for release in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = 256 * 1024 + 12;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            let probe = ctx
                .inflate_probe(root, 12, false)
                .expect("probe")
                .expect("evidence");
            if release {
                drop(probe);
            }
            let result = ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "probe output lifetime",
            );
            if release {
                assert!(result.is_ok());
            } else {
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 12)
                );
            }
        }
    }

    #[test]
    fn deflate_probe_propagates_work_and_scoped_refusals() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"Document.xml").expect("encode");
        let bytes = encoder.finish().expect("finish");
        for dimension in [
            ResourceDimension::WorkUnits,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                _ => panic!("test dimension"),
            }
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
            assert!(
                matches!(ctx.inflate_probe(root, 12, false), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
            );
        }
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let (output, storage) = ctx
            .inflate_probe(root, 12, false)
            .expect("probe")
            .expect("evidence");
        assert_eq!(output, b"Document.xml");
        drop((output, storage));
        assert!(ctx.inflate_probe(root, 11, false).expect("probe").is_none());
    }
}
