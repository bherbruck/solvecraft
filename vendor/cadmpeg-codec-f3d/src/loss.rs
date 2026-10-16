// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Fusion `.f3d` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`F3dLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`F3dLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code so the two cannot drift apart across sites, and it leaves only
//! the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `f3d` namespace.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};

/// A stable, machine-readable identifier for one Fusion `.f3d` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`F3dLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum F3dLossCode {
    /// Payload-bearing Design dimension companions have no typed locus frame.
    DimensionCompanionUntyped,
    /// Design configuration JSON members have no assigned neutral semantics.
    ConfigurationMemberUnassigned,
    /// Design configuration rules have no unambiguous activation target.
    ConfigurationRuleUnbound,
    /// Design configuration parameter overrides have no parameter identity.
    ConfigurationParameterOverrideUnbound,
    /// Design configuration feature suppressions have no feature identity.
    ConfigurationFeatureSuppressionUnbound,
    /// Non-root `ACT` component links stay source-only; product role is unresolved.
    ActComponentLinkUnresolved,
    /// An F3Z drawing root was omitted while its derived 3D model transferred.
    DrawingDocumentOmitted,
    /// ASM history binding work exceeded the decoder safety budget.
    HistoryBindingBudgetExceeded,
    /// An ASM history span stays opaque because record framing failed.
    HistoryRecordFramingFailed,
    /// Design body-map pairs do not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Reference-image timeline objects retain native Canvas records.
    ReferenceImageNativeRetained,
    /// Decal timeline objects retain native image and mapping records.
    DecalNativeRetained,
    /// Source parametric edge references marked lost have no independent proof.
    EdgeReferenceLostUnrepaired,
    /// Feature scopes have no complete neutral feature definition.
    FeatureDefinitionIncomplete,
    /// Decoded feature scopes have no neutral construction-history feature.
    FeatureScopeUnprojected,
    /// A decoded native feature envelope has no interpreted specialized payload.
    FeatureScopePayloadUninterpreted,
    /// Decoded Design parameters have no neutral parameter.
    ParameterUnprojected,
    /// Design parameter owner bindings have no recognized feature scope.
    ParameterOwnerUnrecognized,
    /// Design parameters retain unit tokens without a settled quantity kind.
    ParameterUnitUntyped,
    /// Material Distance properties retain unit tags without a length conversion.
    MaterialDistanceUnitUntyped,
    /// Parameter expression symbols name same-stream parameters without an edge.
    ParameterExpressionUnbound,
    /// Feature history-state dependency links were not projected.
    HistoryDependencyUnprojected,
    /// Feature history-state dependency links have multiple source scopes.
    HistoryDependencyAmbiguous,
    /// A historical face selection cannot bind uniquely to emitted input topology members.
    HistoricalFaceSelectionUnbound,
    /// Sketch relations retain native operands; no unique neutral relation.
    SketchRelationNativeRetained,
    /// Sketch dimensions retain native operands; no unique neutral dimension.
    SketchDimensionNativeRetained,
    /// Decoded Sketch placements have no neutral sketch.
    SketchPlacementUnprojected,
    /// Decoded sketch points have no neutral sketch entity.
    SketchPointUnprojected,
    /// Decoded sketch curves have no neutral sketch entity.
    SketchCurveUnprojected,
    /// Decoded sketch surfaces have no neutral spatial sketch entity.
    SketchSurfaceUnprojected,
    /// Decoded sketch text records have no neutral sketch entity.
    SketchTextUnprojected,
    /// Decoded sketch relations have no neutral constraint.
    SketchRelationUnprojected,
    /// Design dimension parameters have no parameter-backed sketch constraint.
    DimensionUnprojected,
    /// Feature profile selections retain native identities; no unique profile.
    FeatureProfileSelectionNative,
    /// Feature path selections retain native identities; no unique path.
    FeaturePathSelectionNative,
    /// Feature face selections retain native candidates; no unique face.
    FeatureFaceSelectionNative,
    /// Legacy face operands use a current active face after no historical slot proof.
    FeatureFaceSelectionActiveSubstituted,
    /// Feature body selections retain native identities; no unique body.
    FeatureBodySelectionNative,
    /// Feature face operands stay unresolved inside historical selections.
    FeatureFaceOperandUnresolved,
    /// Edge-treatment selections retain native recipes; no historical edge.
    FeatureEdgeSelectionNative,
    /// Edge-treatment operands stay unresolved inside historical selections.
    FeatureEdgeOperandUnresolved,
    /// Edge-treatment selections are unresolved because source edges were lost.
    FeatureEdgeSelectionLost,
    /// Design-referenced BREP blobs could not be decoded.
    BrepBlobUndecoded,
    /// A mesh geometry container decoded without a complete Design body join.
    MeshContainerUnjoined,
    /// A mesh geometry container was not decoded.
    MeshContainerUndecoded,
    /// A Design mesh body names a container that did not uniquely join.
    MeshContainerMissing,
    /// Mesh attribute channels have no settled stored-element layout.
    MeshAttributeNotTransferred,
    /// The external-reference table was present but not decoded.
    XrefTableUndecoded,
    /// A typed occurrence placement named a role but failed its payload grammar.
    XrefPlacementUndecoded,
    /// Structured placement records were superseded by scope-bound carriers.
    XrefPlacementSuperseded,
    /// Mesh body geometry stores vertex coordinates at f32 precision.
    MeshVertexPrecisionReduced,
    /// A bodyless design's sketches or reference images are its complete carrier.
    BodylessDesignCarrier,
    /// An assembly document's geometry lives in external references.
    AssemblyComponentsExternal,
    /// Spline surface records were decoded into NURBS carriers from cache.
    NurbsSurfaceCarrier,
    /// Procedural curve records were decoded into NURBS carriers from cache.
    NurbsCurveCarrier,
    /// Faces were omitted because their required surface reference was dangling.
    FaceSurfaceReferenceDangling,
    /// Faces rest on spline or procedural surfaces whose shape was not decoded.
    SurfaceShapeNotDecoded,
    /// Faces use zero-payload `mesh_surface` sentinels; exact surfaces are absent.
    MeshSurfaceSentinel,
    /// Edges reference a procedural 3D curve with no decodable B-spline cache.
    ProceduralCurveUndecoded,
    /// Coedges carry a UV pcurve reference with no decodable 2D carrier.
    PcurveUndecoded,
    /// Rolling-ball blend definitions resolved only one of two native supports.
    BlendSupportPartial,
    /// Solved-record application or refinement records were not transferred.
    SolvedRecordUntyped,
    /// Materials and appearances were not transferred.
    MaterialNotTransferred,
    /// ASM BREP geometry was not transferred from the container.
    GeometryNotTransferred,
    /// B-rep topology graph was not built from the container.
    TopologyNotTransferred,
    /// No ASM BREP stream was found or selected as the document geometry stream.
    MissingGeometryStream,
    /// An `XREF` path cycles through a member already on the resolution stack.
    XrefCycle,
    /// An `XREF` member is not present in the archive.
    XrefMemberMissing,
    /// An `XREF` member failed to decode.
    XrefMemberUndecoded,
    /// T-spline records were retained without typed semantics.
    TsplineRecordUntyped,
    /// A T-spline control cage was not decoded.
    TsplineCageUndecoded,
    /// Preserved source image required for a byte-exact write was unavailable.
    SourcePreservedImageUnavailable,
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// The selected write target differs from the same-format source dialect.
    /// Export omits records named by the counted loss message.
    WriterAppearancePropertiesOmitted,
    SourceDialectDisplaced,
    /// An embedded kernel carrier was read with an unverified ACIS grammar.
    KernelDialectUnverified,
    /// An embedded kernel carrier could not be framed for dialect inspection.
    KernelCarrierUnparseable,
    /// Two F3Z member layers resolved to the same core dialect identity.
    DialectLayerCollision,
    /// A framed text B-rep carries malformed header metadata or tolerances.
    TextHeaderNoncanonical,
    /// An optional kernel tolerance cannot enter the canonical document units.
    KernelHeaderToleranceUnresolved,
    /// An independently framed binary kernel product string is unreadable.
    KernelHeaderMetadataUnresolved,
    /// A bounded descriptive manifest payload is unreadable.
    ManifestMetadataUnresolved,
    /// An optional bounded archive payload is unreadable.
    ArchiveEntryUnreadable,
}

impl F3dLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [F3dLossCode] = &[
        Self::DimensionCompanionUntyped,
        Self::ConfigurationMemberUnassigned,
        Self::ConfigurationRuleUnbound,
        Self::ConfigurationParameterOverrideUnbound,
        Self::ConfigurationFeatureSuppressionUnbound,
        Self::ActComponentLinkUnresolved,
        Self::DrawingDocumentOmitted,
        Self::HistoryBindingBudgetExceeded,
        Self::HistoryRecordFramingFailed,
        Self::DesignBodyBindingUnresolved,
        Self::ReferenceImageNativeRetained,
        Self::DecalNativeRetained,
        Self::EdgeReferenceLostUnrepaired,
        Self::FeatureDefinitionIncomplete,
        Self::FeatureScopeUnprojected,
        Self::FeatureScopePayloadUninterpreted,
        Self::ParameterUnprojected,
        Self::ParameterOwnerUnrecognized,
        Self::ParameterUnitUntyped,
        Self::MaterialDistanceUnitUntyped,
        Self::ParameterExpressionUnbound,
        Self::HistoryDependencyUnprojected,
        Self::HistoryDependencyAmbiguous,
        Self::HistoricalFaceSelectionUnbound,
        Self::SketchRelationNativeRetained,
        Self::SketchDimensionNativeRetained,
        Self::SketchPlacementUnprojected,
        Self::SketchPointUnprojected,
        Self::SketchCurveUnprojected,
        Self::SketchSurfaceUnprojected,
        Self::SketchTextUnprojected,
        Self::SketchRelationUnprojected,
        Self::DimensionUnprojected,
        Self::FeatureProfileSelectionNative,
        Self::FeaturePathSelectionNative,
        Self::FeatureFaceSelectionNative,
        Self::FeatureFaceSelectionActiveSubstituted,
        Self::FeatureBodySelectionNative,
        Self::FeatureFaceOperandUnresolved,
        Self::FeatureEdgeSelectionNative,
        Self::FeatureEdgeOperandUnresolved,
        Self::FeatureEdgeSelectionLost,
        Self::BrepBlobUndecoded,
        Self::MeshContainerUnjoined,
        Self::MeshContainerUndecoded,
        Self::MeshContainerMissing,
        Self::MeshAttributeNotTransferred,
        Self::XrefTableUndecoded,
        Self::XrefPlacementUndecoded,
        Self::XrefPlacementSuperseded,
        Self::MeshVertexPrecisionReduced,
        Self::BodylessDesignCarrier,
        Self::AssemblyComponentsExternal,
        Self::NurbsSurfaceCarrier,
        Self::NurbsCurveCarrier,
        Self::FaceSurfaceReferenceDangling,
        Self::SurfaceShapeNotDecoded,
        Self::MeshSurfaceSentinel,
        Self::ProceduralCurveUndecoded,
        Self::PcurveUndecoded,
        Self::BlendSupportPartial,
        Self::SolvedRecordUntyped,
        Self::MaterialNotTransferred,
        Self::GeometryNotTransferred,
        Self::TopologyNotTransferred,
        Self::MissingGeometryStream,
        Self::XrefCycle,
        Self::XrefMemberMissing,
        Self::XrefMemberUndecoded,
        Self::TsplineRecordUntyped,
        Self::TsplineCageUndecoded,
        Self::SourcePreservedImageUnavailable,
        Self::SourceDialectUnverified,
        Self::WriterAppearancePropertiesOmitted,
        Self::SourceDialectDisplaced,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::DialectLayerCollision,
        Self::TextHeaderNoncanonical,
        Self::KernelHeaderToleranceUnresolved,
        Self::KernelHeaderMetadataUnresolved,
        Self::ManifestMetadataUnresolved,
        Self::ArchiveEntryUnreadable,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::DimensionCompanionUntyped => "dimension.companion-untyped",
            Self::ConfigurationMemberUnassigned => "configuration.member-unassigned",
            Self::ConfigurationRuleUnbound => "configuration.rule-unbound",
            Self::ConfigurationParameterOverrideUnbound => {
                "configuration.parameter-override-unbound"
            }
            Self::ConfigurationFeatureSuppressionUnbound => {
                "configuration.feature-suppression-unbound"
            }
            Self::ActComponentLinkUnresolved => "assembly.act-component-link-unresolved",
            Self::DrawingDocumentOmitted => "drawing.document-omitted",
            Self::HistoryBindingBudgetExceeded => "history.binding-budget-exceeded",
            Self::HistoryRecordFramingFailed => "history.record-framing-failed",
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::ReferenceImageNativeRetained => "appearance.reference-image-native",
            Self::DecalNativeRetained => "appearance.decal-native",
            Self::EdgeReferenceLostUnrepaired => "feature.edge-reference-lost",
            Self::FeatureDefinitionIncomplete => "feature.definition-incomplete",
            Self::FeatureScopePayloadUninterpreted => "feature.scope-payload-uninterpreted",
            Self::FeatureScopeUnprojected => "feature.scope-unprojected",
            Self::ParameterUnprojected => "parameter.unprojected",
            Self::ParameterOwnerUnrecognized => "parameter.owner-unrecognized",
            Self::ParameterUnitUntyped => "parameter.unit-untyped",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::ParameterExpressionUnbound => "parameter.expression-unbound",
            Self::HistoryDependencyUnprojected => "history.dependency-unprojected",
            Self::HistoricalFaceSelectionUnbound => "history.face-selection-unbound",
            Self::HistoryDependencyAmbiguous => "history.dependency-ambiguous",
            Self::SketchRelationNativeRetained => "sketch.relation-native",
            Self::SketchDimensionNativeRetained => "sketch.dimension-native",
            Self::SketchPlacementUnprojected => "sketch.placement-unprojected",
            Self::SketchPointUnprojected => "sketch.point-unprojected",
            Self::SketchCurveUnprojected => "sketch.curve-unprojected",
            Self::SketchSurfaceUnprojected => "sketch.surface-unprojected",
            Self::SketchTextUnprojected => "sketch.text-unprojected",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::DimensionUnprojected => "dimension.unprojected",
            Self::FeatureProfileSelectionNative => "feature.profile-selection-native",
            Self::FeaturePathSelectionNative => "feature.path-selection-native",
            Self::FeatureFaceSelectionNative => "feature.face-selection-native",
            Self::FeatureFaceSelectionActiveSubstituted => {
                "feature.face-selection-active-substituted"
            }
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureFaceOperandUnresolved => "feature.face-operand-unresolved",
            Self::FeatureEdgeSelectionNative => "feature.edge-selection-native",
            Self::FeatureEdgeOperandUnresolved => "feature.edge-operand-unresolved",
            Self::FeatureEdgeSelectionLost => "feature.edge-selection-lost",
            Self::BrepBlobUndecoded => "geometry.brep-blob-undecoded",
            Self::MeshContainerUnjoined => "mesh.container-unjoined",
            Self::MeshContainerUndecoded => "mesh.container-undecoded",
            Self::MeshContainerMissing => "mesh.container-missing",
            Self::MeshAttributeNotTransferred => "mesh.attribute-not-transferred",
            Self::XrefTableUndecoded => "xref.table-undecoded",
            Self::XrefPlacementUndecoded => "xref.placement-undecoded",
            Self::XrefPlacementSuperseded => "xref.placement-superseded",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::BodylessDesignCarrier => "design.bodyless-carrier",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::NurbsSurfaceCarrier => "geometry.nurbs-surface-carrier",
            Self::NurbsCurveCarrier => "geometry.nurbs-curve-carrier",
            Self::FaceSurfaceReferenceDangling => "geometry.face-surface-dangling",
            Self::SurfaceShapeNotDecoded => "geometry.surface-shape-not-decoded",
            Self::MeshSurfaceSentinel => "geometry.mesh-surface-sentinel",
            Self::ProceduralCurveUndecoded => "geometry.procedural-curve-undecoded",
            Self::PcurveUndecoded => "geometry.pcurve-undecoded",
            Self::BlendSupportPartial => "geometry.blend-support-partial",
            Self::SolvedRecordUntyped => "geometry.solved-record-untyped",
            Self::MaterialNotTransferred => "material.not-transferred",
            Self::GeometryNotTransferred => "geometry.not-transferred",
            Self::TopologyNotTransferred => "topology.not-transferred",
            Self::MissingGeometryStream => "container.missing-geometry-stream",
            Self::XrefCycle => "xref.cycle",
            Self::XrefMemberMissing => "xref.member-missing",
            Self::XrefMemberUndecoded => "xref.member-undecoded",
            Self::TsplineRecordUntyped => "tspline.record-untyped",
            Self::TsplineCageUndecoded => "tspline.cage-undecoded",
            Self::SourcePreservedImageUnavailable => "source.preserved-image-unavailable",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::WriterAppearancePropertiesOmitted => "writer.appearance-properties-omitted",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::TextHeaderNoncanonical => "brep.text-header-noncanonical",
            Self::KernelHeaderToleranceUnresolved => "brep.header-tolerance-unresolved",
            Self::KernelHeaderMetadataUnresolved => "brep.header-metadata-unresolved",
            Self::ManifestMetadataUnresolved => "source.manifest-metadata-unresolved",
            Self::ArchiveEntryUnreadable => "source.archive-entry-unreadable",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::WriterAppearancePropertiesOmitted => Severity::Warning,
            Self::BodylessDesignCarrier
            | Self::AssemblyComponentsExternal
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => Severity::Info,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryRecordFramingFailed
            | Self::MeshContainerUndecoded
            | Self::MissingGeometryStream
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::TsplineCageUndecoded => Severity::Error,
            Self::GeometryNotTransferred
            | Self::TopologyNotTransferred
            | Self::SourcePreservedImageUnavailable => Severity::Blocking,
            Self::DimensionCompanionUntyped
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ActComponentLinkUnresolved
            | Self::DrawingDocumentOmitted
            | Self::DesignBodyBindingUnresolved
            | Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureScopeUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::MaterialDistanceUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchRelationUnprojected
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::BrepBlobUndecoded
            | Self::MeshContainerUnjoined
            | Self::MeshContainerMissing
            | Self::MeshAttributeNotTransferred
            | Self::XrefTableUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefPlacementSuperseded
            | Self::MeshVertexPrecisionReduced
            | Self::FaceSurfaceReferenceDangling
            | Self::SurfaceShapeNotDecoded
            | Self::ProceduralCurveUndecoded
            | Self::PcurveUndecoded
            | Self::BlendSupportPartial
            | Self::SolvedRecordUntyped
            | Self::MaterialNotTransferred
            | Self::TsplineRecordUntyped
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::KernelDialectUnverified
            | Self::KernelCarrierUnparseable
            | Self::DialectLayerCollision
            | Self::TextHeaderNoncanonical
            | Self::KernelHeaderToleranceUnresolved
            | Self::KernelHeaderMetadataUnresolved
            | Self::ManifestMetadataUnresolved
            | Self::ArchiveEntryUnreadable => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::WriterAppearancePropertiesOmitted => LossTaxonomy::AppearanceReduced,
            Self::DimensionCompanionUntyped
            | Self::HistoryRecordFramingFailed
            | Self::SolvedRecordUntyped
            | Self::KernelCarrierUnparseable
            | Self::TsplineRecordUntyped => LossTaxonomy::RecordNotTyped,
            Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::DrawingDocumentOmitted
            | Self::XrefTableUndecoded => LossTaxonomy::MetadataNotTransferred,
            Self::ActComponentLinkUnresolved
            | Self::AssemblyComponentsExternal
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefPlacementSuperseded => LossTaxonomy::AssemblyComponentsExternal,
            Self::HistoryBindingBudgetExceeded
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureScopeUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchRelationUnprojected
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost => LossTaxonomy::FeatureHistoryRetained,
            Self::MaterialDistanceUnitUntyped => LossTaxonomy::MaterialNotTransferred,
            Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::PcurveUndecoded => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::MeshAttributeNotTransferred => LossTaxonomy::AttributesNotTransferred,
            Self::BrepBlobUndecoded
            | Self::SurfaceShapeNotDecoded
            | Self::GeometryNotTransferred
            | Self::TsplineCageUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::MeshContainerUnjoined | Self::MeshContainerMissing => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::MeshContainerUndecoded | Self::DialectLayerCollision => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::MeshVertexPrecisionReduced => LossTaxonomy::MeshVertexPrecision,
            Self::BodylessDesignCarrier
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => LossTaxonomy::CarrierSummary,
            Self::ProceduralCurveUndecoded | Self::BlendSupportPartial => {
                LossTaxonomy::ProceduralReduced
            }
            Self::MaterialNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::TopologyNotTransferred => LossTaxonomy::TopologyNotTransferred,
            Self::MissingGeometryStream => LossTaxonomy::MissingGeometryStream,
            Self::SourcePreservedImageUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TextHeaderNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::KernelHeaderMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ManifestMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ArchiveEntryUnreadable => LossTaxonomy::RecordNotTyped,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(
            cadmpeg_ir::loss_namespace!("f3d"),
            self.code(),
            self.shared_taxonomy(),
        )
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `f3d/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::F3dLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "dimension.companion-untyped",
                "configuration.member-unassigned",
                "configuration.rule-unbound",
                "configuration.parameter-override-unbound",
                "configuration.feature-suppression-unbound",
                "assembly.act-component-link-unresolved",
                "drawing.document-omitted",
                "history.binding-budget-exceeded",
                "history.record-framing-failed",
                "design.body-binding-unresolved",
                "appearance.reference-image-native",
                "appearance.decal-native",
                "feature.edge-reference-lost",
                "feature.definition-incomplete",
                "feature.scope-unprojected",
                "feature.scope-payload-uninterpreted",
                "parameter.unprojected",
                "parameter.owner-unrecognized",
                "parameter.unit-untyped",
                "material.distance-unit-untyped",
                "parameter.expression-unbound",
                "history.dependency-unprojected",
                "history.dependency-ambiguous",
                "history.face-selection-unbound",
                "sketch.relation-native",
                "sketch.dimension-native",
                "sketch.placement-unprojected",
                "sketch.point-unprojected",
                "sketch.curve-unprojected",
                "sketch.surface-unprojected",
                "sketch.text-unprojected",
                "sketch.relation-unprojected",
                "dimension.unprojected",
                "feature.profile-selection-native",
                "feature.path-selection-native",
                "feature.face-selection-native",
                "feature.face-selection-active-substituted",
                "feature.body-selection-native",
                "feature.face-operand-unresolved",
                "feature.edge-selection-native",
                "feature.edge-operand-unresolved",
                "feature.edge-selection-lost",
                "geometry.brep-blob-undecoded",
                "mesh.container-unjoined",
                "mesh.container-undecoded",
                "mesh.container-missing",
                "mesh.attribute-not-transferred",
                "xref.table-undecoded",
                "xref.placement-undecoded",
                "xref.placement-superseded",
                "mesh.vertex-precision-reduced",
                "design.bodyless-carrier",
                "assembly.components-external",
                "geometry.nurbs-surface-carrier",
                "geometry.nurbs-curve-carrier",
                "geometry.face-surface-dangling",
                "geometry.surface-shape-not-decoded",
                "geometry.mesh-surface-sentinel",
                "geometry.procedural-curve-undecoded",
                "geometry.pcurve-undecoded",
                "geometry.blend-support-partial",
                "geometry.solved-record-untyped",
                "material.not-transferred",
                "geometry.not-transferred",
                "topology.not-transferred",
                "container.missing-geometry-stream",
                "xref.cycle",
                "xref.member-missing",
                "xref.member-undecoded",
                "tspline.record-untyped",
                "tspline.cage-undecoded",
                "source.preserved-image-unavailable",
                "source.dialect-unverified",
                "writer.appearance-properties-omitted",
                "target.source-dialect-displaced",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.dialect-layer-collision",
                "brep.text-header-noncanonical",
                "brep.header-tolerance-unresolved",
                "brep.header-metadata-unresolved",
                "source.manifest-metadata-unresolved",
                "source.archive-entry-unreadable",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in F3dLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in F3dLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
