// SPDX-License-Identifier: Apache-2.0
//! Admitted indices for decoded PMI annotations.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition, PmiTarget};

/// Fields supplied before the source record ID is minted for the IR arena.
pub(super) struct AnnotationDraft {
    pub(super) name: Option<String>,
    pub(super) targets: Vec<PmiTarget>,
    pub(super) visible: Option<bool>,
    pub(super) definition: PmiDefinition,
}

/// An index minted by insertion into the PMI arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AnnotationIndex(usize);

impl AnnotationIndex {
    /// The inserted annotation’s arena position.
    pub(super) fn get(self) -> usize {
        self.0
    }
}

/// STEP records mapped to inserted PMI annotations.
#[derive(Default)]
pub(super) struct Annotations {
    indices: BTreeMap<u64, AnnotationIndex>,
}

impl Annotations {
    /// Insert an annotation and return its arena index.
    pub(super) fn push(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        id: u64,
        draft: AnnotationDraft,
    ) -> Result<AnnotationIndex, CodecError> {
        ctx.reserve_vec(&mut ir.model.pmi, 1, "step_pmi_annotation_arena")?;
        let index = AnnotationIndex(ir.model.pmi.len());
        ir.model.pmi.push(PmiAnnotation {
            id: super::pmi_id(id),
            name: draft.name.filter(|value| !value.is_empty()),
            targets: draft.targets,
            visible: draft.visible,
            definition: draft.definition,
        });
        ctx.insert_btree_map(&mut self.indices, id, index, "step_pmi_annotation_index")?;
        Ok(index)
    }

    /// The inserted annotation index for a STEP record.
    pub(super) fn get(&self, id: u64) -> Option<AnnotationIndex> {
        self.indices.get(&id).copied()
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::pmi::PmiDefinition;

    use super::{AnnotationDraft, Annotations};

    fn refusal_at(limit: u64) -> CodecError {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        let mut ir = CadIr::empty();
        Annotations::default()
            .push(
                &ctx,
                &mut ir,
                1,
                AnnotationDraft {
                    name: None,
                    targets: Vec::new(),
                    visible: None,
                    definition: PmiDefinition::Datum {
                        identification: String::new(),
                    },
                },
            )
            .expect_err("limit must refuse one annotation")
    }

    #[test]
    fn pmi_annotation_arena_refuses_collection_limit() {
        assert!(matches!(
            refusal_at(0),
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_pmi_annotation_arena"
        ));
    }

    #[test]
    fn pmi_annotation_index_refuses_collection_limit() {
        assert!(matches!(
            refusal_at(1),
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_pmi_annotation_index"
        ));
    }
}
