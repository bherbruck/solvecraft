// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for STEP / `.stp` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`StepLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`StepLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `step` namespace.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = match LossNamespace::new("step") {
    Ok(namespace) => namespace,
    Err(_) => panic!("reserved codec namespace"),
};

/// A stable, machine-readable identifier for one STEP / `.stp` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`StepLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StepLossCode {
    /// Parser recovered noncanonical Part 21 syntax.
    ParseNoncanonicalSyntax,
    /// A bounded DATA record or its dependent could not be interpreted.
    ParseRecordOmitted,
    /// A bounded noncanonical prefix remains source-only.
    ParsePreambleOmitted,
    /// A pre-standard exchange used the Part 21 instance grammar.
    ParseDraftGrammar,
    /// Readable DATA ends before a complete closing envelope.
    ParseEnvelopeIncomplete,
    /// ZIP metadata violates the Part 21 profile without preventing root decoding.
    ContainerMemberNoncanonical,
    /// A decode stage surfaced a per-record warning.
    DecodeWarning,
    /// Byte accounting left source bytes unclassified.
    ByteAccountingUnclassified,
    /// Named entity instances were retained as opaque records.
    OpaqueRecordPreserved,
    /// A metadata string field failed to decode.
    MetadataStringInvalid,
    /// A `FILE_SCHEMA` object identifier component is out of range.
    SchemaObjectIdentifierOutOfRange,
    /// The declared Part 21 implementation level has no implemented grammar.
    ImplementationLevelUnverified,
    /// Header metadata deviates from the format without preventing decoding.
    HeaderMetadataNoncanonical,
    /// The declared `FILE_SCHEMA` identifier satisfies no declared dialect.
    SourceDialectUnverified,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
    /// An attribute string field failed to decode.
    AttributeStringInvalid,
    /// `AXIS2_PLACEMENT_3D` reference direction was parallel to its axis.
    PlacementReferenceInferred,
    /// Geometry records belong to representations with conflicting units.
    ConflictingRepresentationUnits,
    /// `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` has an ambiguous length measure.
    UncertaintyLengthAmbiguous,
    /// `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` has no resolvable length measure.
    UncertaintyLengthUnresolved,
    /// The document length unit did not resolve.
    DocumentLengthUnitUnresolved,
    /// The document plane-angle unit did not resolve.
    DocumentAngleUnitUnresolved,
    /// A LINE parameter scale did not resolve.
    LineParameterScaleUnresolved,
    /// A topology root was rejected with a failure message.
    TopologyRootRejected,
    /// A topology root does not resolve to a complete connected graph.
    TopologyRootIncomplete,
    /// An oriented shell omits the derived `cfs_faces` slot.
    OrientedShellOmitsCfsFaces,
    /// A `SEAM_EDGE` has no decoded pcurve on its edge curve and face surface.
    SeamEdgePcurveUnresolved,
    /// An edge has no decoded surface or curve carrier for a pcurve.
    EdgeNoSurfaceOrCurveForPcurve,
    /// A single optional pcurve is not endpoint-continuous with the edge.
    PcurveEndpointsDiscontinuous,
    /// A single endpoint-continuous pcurve fails the bounded model-space locus witness.
    PcurveLocusDiscontinuous,
    /// A finite non-seam pcurve witness did not prove global point-set and direction fidelity.
    PcurveGlobalFidelityUnproved,
    /// Several pcurves associate with a surface and none selects uniquely.
    PcurveAssociationAmbiguous,
    /// Pcurve candidates exist but the source surface or curve is unresolved.
    PcurveCandidatesCarrierUnresolved,
    /// A face has more than one `FACE_OUTER_BOUND`.
    FaceMultipleOuterBounds,
    /// A source shell contains disconnected face components.
    ShellDisconnectedFaces,
    /// A NAUO has no resolved occurrence transform.
    NauoPlacementUnresolved,
    /// A NAUO has competing resolved placement candidates.
    NauoPlacementAmbiguous,
    /// A body has conflicting standalone `MAPPED_ITEM` placements.
    BodyConflictingMappedPlacements,
    /// A presentation annotation has several text carriers and no order.
    PresentationAnnotationTextUnordered,
    /// A presentation annotation has several placement carriers and no unique placement.
    PresentationAnnotationPlacementAmbiguous,
    /// A dimensional characteristic has several named nominal measures.
    DimensionalNominalAmbiguous,
    /// A dimensional characteristic has several unnamed measure values.
    DimensionalUnnamedMeasureAmbiguous,
    /// A PMI length measure unit scale did not resolve.
    PmiLengthUnitUnresolved,
    /// A PMI angle measure unit scale did not resolve.
    PmiAngleUnitUnresolved,
    /// A datum system has inconsistent precedence compartments.
    PmiDatumSystemInvalid,
    /// Independent styled items assign conflicting scalar colors.
    ConflictingScalarColors,
    /// A surface style usage has an invalid `surface_side` enumeration value.
    SurfaceSideInvalid,
    /// A surface rendering has multiple transparency properties.
    SurfaceTransparencyConflict,
    /// A context-dependent style has no matching neutral presentation context.
    ContextDependentStyleUnresolved,
    /// A drawing record has too few parameters and was retained opaque.
    DrawingRecordTooFewParameters,
    /// A drawing's position in the stored order passes the stated order width.
    DrawingOrderUnstatable,
    /// A drawing relationship references a source-typed record without identity.
    DrawingRelationshipUntypedTarget,
    /// A drawing relationship references a source record with multiple identities.
    DrawingRelationshipTargetAmbiguous,
    /// A drawing sheet usage has no resolvable drawing revision.
    DrawingSheetRevisionUnresolved,
    /// A drawing revision usage has no resolvable sheet revision.
    DrawingRevisionSheetUnresolved,
    /// A draughting model association references a typed semantic definition without identity.
    DraughtingSemanticDefinitionUntyped,
    /// A draughting model association references a source-typed item without identity.
    DraughtingAssociatedItemUntyped,
    /// A body-representation tessellation item does not bind to exactly one decoded body.
    TessellationItemBodyUnresolved,
    /// A tessellation payload fails numeric or structural admission.
    TessellationInvalidPayload,
    /// A tessellation item lacks an exact body-container or tessellated-representation declaration.
    TessellationItemUndeclared,
    /// A repositioned tessellation item has no valid placement.
    TessellationPlacementUnresolved,
    /// A detached tessellation item has multiple distinct repositioning placements.
    TessellationPlacementAmbiguous,
    /// A geometric validation measure unit scale did not resolve.
    ValidationMeasureUnitUnresolved,
    /// The IR document has bodies but no exportable solids.
    NoExportableSolids,
    /// A layer item has no writable STEP carrier.
    LayerItemWithoutCarrier,
    /// The assembly occurrence graph is invalid.
    AssemblyGraphInvalid,
    /// A root occurrence placement or scale is not representable.
    RootOccurrencePlacementNotRepresentable,
    /// An occurrence has an unresolved parent.
    OccurrenceUnresolvedParent,
    /// An occurrence has no local product definition.
    OccurrenceNoLocalProduct,
    /// An occurrence placement is not rigid.
    OccurrencePlacementNotRigid,
    /// A body carries a non-rigid transform.
    BodyNonRigidTransform,
    /// Hidden body visibility assignments are unsupported by the target schema.
    HiddenBodyVisibilityUnsupported,
    /// Hidden appearance binding visibility assignments are unsupported by the target schema.
    HiddenAppearanceVisibilityUnsupported,
    /// Hidden presentation layer visibility assignments are unsupported by the target schema.
    HiddenPresentationLayerVisibilityUnsupported,
    /// Hidden PMI annotation visibility assignments are unsupported by the target schema.
    HiddenPmiVisibilityUnsupported,
    /// A wire shell has free vertices without an edge-based STEP carrier.
    WireShellFreeVertices,
    /// Bodies have no owning product representation.
    BodyWithoutProductRepresentation,
    /// Solid and sheet shells omit wire edges and free vertices.
    SurfaceShellWireTopologyOmitted,
    /// Tessellations require an AP242 target.
    TessellationRequiresAp242,
    /// Tessellation feature-edge classification is not represented.
    TessellationFeatureEdges,
    /// Tessellation corner normals are not represented.
    TessellationCornerNormals,
    /// Tessellation triangle groups are not represented.
    TessellationTriangleGroups,
    /// Tessellation texture assignments are not represented.
    TessellationTextureAssignments,
    /// A tessellation has invalid vertex, index, or normal cardinality.
    TessellationInvalidCardinality,
    /// A tessellation body has no writable AP242 tessellation link.
    TessellationBodyLinkUnwritable,
    /// Tessellation metadata was reduced because STEP cannot represent it.
    TessellationMetadataReduced,
    /// Topology is not reachable from any emitted region shape item.
    TopologyUnreachableFromRegion,
    /// Signed or self-intersecting analytic surfaces were normalized.
    AnalyticSurfaceNormalized,
    /// Elliptical cone surfaces were reduced to circular `CONICAL_SURFACE`.
    EllipticalConeReduced,
    /// Edges have no typed 3D curve or carry an unsupported transform.
    CurvelessEdgeOmitted,
    /// Faces rest on an unknown or STEP-unsupported surface.
    UnknownSurfaceFaceOmitted,
    /// Geometry carriers were not written.
    GeometryCarrierNotWritten,
    /// Coedge pcurve carriers use unwritable geometry or surface references.
    PcurveCarrierUnwritable,
    /// Assembly occurrences were omitted because the parent has no local product.
    AssemblyOccurrenceOmittedNoParentProduct,
    /// Regions have no shell list and were not written.
    RegionNoShellList,
    /// Wire regions had no writable connected edge set.
    WireRegionNoConnectedEdgeSet,
    /// Wire region or shell relations referenced missing shell records.
    WireRegionMissingShell,
    /// Hidden bodies had no emitted STEP item and were omitted from INVISIBILITY.
    HiddenBodyOmitted,
    /// Hidden presentation layers had no emitted STEP assignment and were omitted from INVISIBILITY.
    HiddenPresentationLayerOmitted,
    /// Appearance bindings reference missing appearance assets.
    AppearanceBindingMissingAsset,
    /// Appearance bindings reference appearances without a base color.
    AppearanceBindingNoBaseColor,
    /// Appearance bindings target one body or face with conflicting styles.
    AppearanceBindingTargetConflict,
    /// Coedge pcurve references have no geometry.
    CoedgePcurveNoGeometry,
    /// Emitted coedge pcurves carry native-only metadata.
    CoedgePcurveNativeMetadata,
    /// Pcurve uses carry native-only parameter metadata.
    PcurveUseNativeMetadata,
    /// Coedge-local 3D curve uses were not represented.
    CoedgeUseCurveNotRepresented,
    /// Topology metadata values were not represented.
    TopologyMetadataNotRepresented,
    /// Subdivision surfaces were omitted.
    SubdOmitted,
    /// Parametric or design records were not represented.
    ParametricDesignRecordsOmitted,
    /// Drawing or presentation records were not represented.
    DrawingPresentationRecordsOmitted,
    /// Semantic annotations were not represented.
    SemanticAnnotationOmitted,
    /// Document assets were not represented.
    DocumentAssetOmitted,
    /// Assembly joints were not represented.
    AssemblyJointOmitted,
    /// Product definitions use a non-part kind.
    ProductNonPartKind,
    /// Product BOM property values were not represented.
    ProductBomPropertyOmitted,
    /// Occurrences reference external product definitions.
    OccurrenceExternalProduct,
    /// Occurrences have no writable product definition.
    OccurrenceNoWritableProduct,
    /// Occurrence metadata values were not represented.
    OccurrenceMetadataOmitted,
    /// PMI annotations were not written.
    PmiAnnotationNotWritten,
    /// Source-object associations were not represented.
    SourceAssociationOmitted,
    /// Uninterpreted passthrough records were not represented.
    PassthroughRecordOmitted,
    /// Display colors had no emitted STEP item.
    DisplayColorUnstyled,
    /// A wire body's direct color alpha was omitted from RGB curve styling.
    WireBodyTransparencyOmitted,
    /// Appearance assets were reduced to `STYLED_ITEM` base colors.
    AppearanceReducedToBaseColor,
    /// Appearance bindings carry source object or channel metadata.
    AppearanceBindingMetadataReduced,
    /// Source attribute records were not written.
    SourceAttributeNotWritten,
    /// Procedural definitions were reduced to solved STEP carriers.
    ProceduralReducedToCarrier,
    /// A procedural definition has no emitted native construction or solved carrier.
    ProceduralDefinitionNotWritten,
    /// Source-native records were not represented.
    SourceNativeRecordOmitted,
    /// A region has no writable outer shell.
    RegionNoWritableOuterShell,
    /// A region omitted a void shell with no writable faces.
    RegionOmittedVoidShell,
    /// A shell omitted an outer face with no writable topology.
    ShellOmittedOuterFace,
    /// A shell omitted a face whose boundary roles are unspecified.
    ShellOmittedUnclassifiedFace,
    /// A face omitted an outer loop with no writable topology.
    FaceOmittedOuterLoop,
    /// A face omitted an inner loop with no writable topology.
    FaceOmittedInnerLoop,
    /// A face omitted a loop whose boundary role is unspecified.
    FaceOmittedUnclassifiedLoop,
    /// A face has no writable bounds.
    FaceNoWritableBounds,
    /// A loop was omitted because its record is missing.
    LoopRecordMissing,
    /// A loop was omitted because its vertex is missing.
    LoopVertexMissing,
    /// A loop omitted a coedge because its record is missing.
    LoopCoedgeRecordMissing,
    /// A loop omitted an edge because the edge is not writable.
    LoopEdgeNotWritable,
    /// A loop has no writable edges.
    LoopNoWritableEdges,
    /// A loop cannot be ordered because a coedge occurs more than once.
    LoopDuplicateCoedge,
    /// A loop cannot be ordered because a coedge is missing.
    LoopCoedgeMissingForOrder,
    /// A loop cannot be ordered because an edge is missing.
    LoopEdgeMissingForOrder,
    /// An edge was omitted because it has no 3D curve.
    EdgeNo3dCurve,
    /// An edge was omitted because its curve is not writable.
    EdgeCurveNotWritable,
    /// An edge was omitted because its curve is unsupported.
    EdgeCurveUnsupported,
    /// A loop has no continuous vertex-to-vertex coedge ordering.
    LoopNoContinuousOrdering,
    /// An edge was omitted because its record is missing.
    EdgeRecordMissing,
    /// An edge was omitted because its start vertex is missing.
    EdgeStartVertexMissing,
    /// An edge was omitted because its end vertex is missing.
    EdgeEndVertexMissing,
}

impl StepLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [StepLossCode] = &[
        Self::ParseNoncanonicalSyntax,
        Self::ParseRecordOmitted,
        Self::ParsePreambleOmitted,
        Self::ParseDraftGrammar,
        Self::ParseEnvelopeIncomplete,
        Self::ContainerMemberNoncanonical,
        Self::DecodeWarning,
        Self::ByteAccountingUnclassified,
        Self::OpaqueRecordPreserved,
        Self::MetadataStringInvalid,
        Self::SchemaObjectIdentifierOutOfRange,
        Self::ImplementationLevelUnverified,
        Self::HeaderMetadataNoncanonical,
        Self::SourceDialectUnverified,
        Self::SourceDialectDisplaced,
        Self::AttributeStringInvalid,
        Self::PlacementReferenceInferred,
        Self::ConflictingRepresentationUnits,
        Self::UncertaintyLengthAmbiguous,
        Self::UncertaintyLengthUnresolved,
        Self::DocumentLengthUnitUnresolved,
        Self::DocumentAngleUnitUnresolved,
        Self::LineParameterScaleUnresolved,
        Self::TopologyRootRejected,
        Self::TopologyRootIncomplete,
        Self::OrientedShellOmitsCfsFaces,
        Self::SeamEdgePcurveUnresolved,
        Self::EdgeNoSurfaceOrCurveForPcurve,
        Self::PcurveEndpointsDiscontinuous,
        Self::PcurveLocusDiscontinuous,
        Self::PcurveGlobalFidelityUnproved,
        Self::PcurveAssociationAmbiguous,
        Self::PcurveCandidatesCarrierUnresolved,
        Self::FaceMultipleOuterBounds,
        Self::ShellDisconnectedFaces,
        Self::NauoPlacementUnresolved,
        Self::NauoPlacementAmbiguous,
        Self::BodyConflictingMappedPlacements,
        Self::PresentationAnnotationTextUnordered,
        Self::PresentationAnnotationPlacementAmbiguous,
        Self::DimensionalNominalAmbiguous,
        Self::DimensionalUnnamedMeasureAmbiguous,
        Self::PmiLengthUnitUnresolved,
        Self::PmiAngleUnitUnresolved,
        Self::PmiDatumSystemInvalid,
        Self::ConflictingScalarColors,
        Self::SurfaceSideInvalid,
        Self::SurfaceTransparencyConflict,
        Self::ContextDependentStyleUnresolved,
        Self::DrawingRecordTooFewParameters,
        Self::DrawingOrderUnstatable,
        Self::DrawingRelationshipUntypedTarget,
        Self::DrawingRelationshipTargetAmbiguous,
        Self::DrawingSheetRevisionUnresolved,
        Self::DrawingRevisionSheetUnresolved,
        Self::DraughtingSemanticDefinitionUntyped,
        Self::DraughtingAssociatedItemUntyped,
        Self::TessellationItemBodyUnresolved,
        Self::TessellationInvalidPayload,
        Self::TessellationItemUndeclared,
        Self::TessellationPlacementUnresolved,
        Self::TessellationPlacementAmbiguous,
        Self::ValidationMeasureUnitUnresolved,
        Self::NoExportableSolids,
        Self::LayerItemWithoutCarrier,
        Self::AssemblyGraphInvalid,
        Self::RootOccurrencePlacementNotRepresentable,
        Self::OccurrenceUnresolvedParent,
        Self::OccurrenceNoLocalProduct,
        Self::OccurrencePlacementNotRigid,
        Self::BodyNonRigidTransform,
        Self::HiddenBodyVisibilityUnsupported,
        Self::HiddenAppearanceVisibilityUnsupported,
        Self::HiddenPresentationLayerVisibilityUnsupported,
        Self::HiddenPmiVisibilityUnsupported,
        Self::WireShellFreeVertices,
        Self::BodyWithoutProductRepresentation,
        Self::SurfaceShellWireTopologyOmitted,
        Self::TessellationRequiresAp242,
        Self::TessellationFeatureEdges,
        Self::TessellationCornerNormals,
        Self::TessellationTriangleGroups,
        Self::TessellationTextureAssignments,
        Self::TessellationInvalidCardinality,
        Self::TessellationBodyLinkUnwritable,
        Self::TessellationMetadataReduced,
        Self::TopologyUnreachableFromRegion,
        Self::AnalyticSurfaceNormalized,
        Self::EllipticalConeReduced,
        Self::CurvelessEdgeOmitted,
        Self::UnknownSurfaceFaceOmitted,
        Self::GeometryCarrierNotWritten,
        Self::PcurveCarrierUnwritable,
        Self::AssemblyOccurrenceOmittedNoParentProduct,
        Self::RegionNoShellList,
        Self::WireRegionNoConnectedEdgeSet,
        Self::WireRegionMissingShell,
        Self::HiddenBodyOmitted,
        Self::HiddenPresentationLayerOmitted,
        Self::AppearanceBindingMissingAsset,
        Self::AppearanceBindingNoBaseColor,
        Self::AppearanceBindingTargetConflict,
        Self::CoedgePcurveNoGeometry,
        Self::CoedgePcurveNativeMetadata,
        Self::PcurveUseNativeMetadata,
        Self::CoedgeUseCurveNotRepresented,
        Self::TopologyMetadataNotRepresented,
        Self::SubdOmitted,
        Self::ParametricDesignRecordsOmitted,
        Self::DrawingPresentationRecordsOmitted,
        Self::SemanticAnnotationOmitted,
        Self::DocumentAssetOmitted,
        Self::AssemblyJointOmitted,
        Self::ProductNonPartKind,
        Self::ProductBomPropertyOmitted,
        Self::OccurrenceExternalProduct,
        Self::OccurrenceNoWritableProduct,
        Self::OccurrenceMetadataOmitted,
        Self::PmiAnnotationNotWritten,
        Self::SourceAssociationOmitted,
        Self::PassthroughRecordOmitted,
        Self::DisplayColorUnstyled,
        Self::WireBodyTransparencyOmitted,
        Self::AppearanceReducedToBaseColor,
        Self::AppearanceBindingMetadataReduced,
        Self::SourceAttributeNotWritten,
        Self::ProceduralReducedToCarrier,
        Self::ProceduralDefinitionNotWritten,
        Self::SourceNativeRecordOmitted,
        Self::RegionNoWritableOuterShell,
        Self::RegionOmittedVoidShell,
        Self::ShellOmittedOuterFace,
        Self::ShellOmittedUnclassifiedFace,
        Self::FaceOmittedOuterLoop,
        Self::FaceOmittedInnerLoop,
        Self::FaceOmittedUnclassifiedLoop,
        Self::FaceNoWritableBounds,
        Self::LoopRecordMissing,
        Self::LoopVertexMissing,
        Self::LoopCoedgeRecordMissing,
        Self::LoopEdgeNotWritable,
        Self::LoopNoWritableEdges,
        Self::LoopDuplicateCoedge,
        Self::LoopCoedgeMissingForOrder,
        Self::LoopEdgeMissingForOrder,
        Self::EdgeNo3dCurve,
        Self::EdgeCurveNotWritable,
        Self::EdgeCurveUnsupported,
        Self::LoopNoContinuousOrdering,
        Self::EdgeRecordMissing,
        Self::EdgeStartVertexMissing,
        Self::EdgeEndVertexMissing,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ContainerMemberNoncanonical => "container.member-noncanonical",
            Self::ParseNoncanonicalSyntax => "parse.noncanonical-syntax",
            Self::ParseRecordOmitted => "parse.record-omitted",
            Self::ParsePreambleOmitted => "parse.preamble-omitted",
            Self::ParseDraftGrammar => "parse.draft-grammar",
            Self::ParseEnvelopeIncomplete => "parse.envelope-incomplete",
            Self::DecodeWarning => "decode.warning",
            Self::ByteAccountingUnclassified => "decode.byte-accounting-unclassified",
            Self::OpaqueRecordPreserved => "decode.opaque-record-preserved",
            Self::MetadataStringInvalid => "metadata.string-invalid",
            Self::SchemaObjectIdentifierOutOfRange => {
                "metadata.schema-object-identifier-out-of-range"
            }
            Self::ImplementationLevelUnverified => "parse.implementation-level-unverified",
            Self::HeaderMetadataNoncanonical => "parse.header-metadata-noncanonical",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::AttributeStringInvalid => "attribute.string-invalid",
            Self::PlacementReferenceInferred => "geometry.placement-reference-inferred",
            Self::ConflictingRepresentationUnits => "geometry.conflicting-representation-units",
            Self::UncertaintyLengthAmbiguous => "geometry.uncertainty-length-ambiguous",
            Self::UncertaintyLengthUnresolved => "geometry.uncertainty-length-unresolved",
            Self::DocumentLengthUnitUnresolved => "geometry.document-length-unit-unresolved",
            Self::DocumentAngleUnitUnresolved => "geometry.document-angle-unit-unresolved",
            Self::LineParameterScaleUnresolved => "geometry.line-parameter-scale-unresolved",
            Self::TopologyRootRejected => "topology.root-rejected",
            Self::TopologyRootIncomplete => "topology.root-incomplete",
            Self::OrientedShellOmitsCfsFaces => "topology.oriented-shell-omits-cfs-faces",
            Self::SeamEdgePcurveUnresolved => "topology.seam-edge-pcurve-unresolved",
            Self::EdgeNoSurfaceOrCurveForPcurve => "topology.edge-no-surface-or-curve-for-pcurve",
            Self::PcurveEndpointsDiscontinuous => "topology.pcurve-endpoints-discontinuous",
            Self::PcurveLocusDiscontinuous => "topology.pcurve-locus-discontinuous",
            Self::PcurveGlobalFidelityUnproved => "topology.pcurve-global-fidelity-unproved",
            Self::PcurveAssociationAmbiguous => "topology.pcurve-association-ambiguous",
            Self::PcurveCandidatesCarrierUnresolved => {
                "topology.pcurve-candidates-carrier-unresolved"
            }
            Self::FaceMultipleOuterBounds => "topology.face-multiple-outer-bounds",
            Self::ShellDisconnectedFaces => "topology.shell-disconnected-faces",
            Self::NauoPlacementUnresolved => "product.nauo-placement-unresolved",
            Self::NauoPlacementAmbiguous => "product.nauo-placement-ambiguous",
            Self::BodyConflictingMappedPlacements => "product.body-conflicting-mapped-placements",
            Self::PresentationAnnotationTextUnordered => {
                "pmi.presentation-annotation-text-unordered"
            }
            Self::PresentationAnnotationPlacementAmbiguous => {
                "pmi.presentation-annotation-placement-ambiguous"
            }
            Self::DimensionalNominalAmbiguous => "pmi.dimensional-nominal-ambiguous",
            Self::DimensionalUnnamedMeasureAmbiguous => "pmi.dimensional-unnamed-measure-ambiguous",
            Self::PmiLengthUnitUnresolved => "pmi.length-unit-unresolved",
            Self::PmiAngleUnitUnresolved => "pmi.angle-unit-unresolved",
            Self::PmiDatumSystemInvalid => "pmi.datum-system-invalid",
            Self::ConflictingScalarColors => "presentation.conflicting-scalar-colors",
            Self::SurfaceSideInvalid => "presentation.surface-side-invalid",
            Self::SurfaceTransparencyConflict => "presentation.surface-transparency-conflict",
            Self::ContextDependentStyleUnresolved => {
                "presentation.context-dependent-style-unresolved"
            }
            Self::DrawingRecordTooFewParameters => "drawing.record-too-few-parameters",
            Self::DrawingOrderUnstatable => "drawing.order-unstatable",
            Self::DrawingRelationshipUntypedTarget => "drawing.relationship-untyped-target",
            Self::DrawingRelationshipTargetAmbiguous => "drawing.relationship-target-ambiguous",
            Self::DrawingSheetRevisionUnresolved => "drawing.sheet-revision-unresolved",
            Self::DrawingRevisionSheetUnresolved => "drawing.revision-sheet-unresolved",
            Self::DraughtingSemanticDefinitionUntyped => {
                "drawing.draughting-semantic-definition-untyped"
            }
            Self::DraughtingAssociatedItemUntyped => "drawing.draughting-associated-item-untyped",
            Self::TessellationInvalidPayload => "tessellation.invalid-payload",
            Self::TessellationItemBodyUnresolved => "tessellation.item-body-unresolved",
            Self::TessellationItemUndeclared => "tessellation.item-undeclared",
            Self::TessellationPlacementUnresolved => "tessellation.placement-unresolved",
            Self::TessellationPlacementAmbiguous => "tessellation.placement-ambiguous",
            Self::ValidationMeasureUnitUnresolved => "validation.measure-unit-unresolved",
            Self::NoExportableSolids => "export.no-exportable-solids",
            Self::LayerItemWithoutCarrier => "layer.item-without-carrier",
            Self::AssemblyGraphInvalid => "assembly.graph-invalid",
            Self::RootOccurrencePlacementNotRepresentable => {
                "occurrence.root-placement-not-representable"
            }
            Self::OccurrenceUnresolvedParent => "occurrence.unresolved-parent",
            Self::OccurrenceNoLocalProduct => "occurrence.no-local-product",
            Self::OccurrencePlacementNotRigid => "occurrence.placement-not-rigid",
            Self::BodyNonRigidTransform => "body.non-rigid-transform",
            Self::HiddenBodyVisibilityUnsupported => "body.hidden-visibility-unsupported",
            Self::HiddenAppearanceVisibilityUnsupported => {
                "presentation.hidden-appearance-visibility-unsupported"
            }
            Self::HiddenPresentationLayerVisibilityUnsupported => {
                "presentation.hidden-layer-visibility-unsupported"
            }
            Self::HiddenPmiVisibilityUnsupported => "pmi.hidden-visibility-unsupported",
            Self::BodyWithoutProductRepresentation => "product.body-without-representation",
            Self::SurfaceShellWireTopologyOmitted => "topology.surface-shell-wire-omitted",
            Self::WireShellFreeVertices => "topology.wire-shell-free-vertices",
            Self::TessellationRequiresAp242 => "tessellation.requires-ap242",
            Self::TessellationFeatureEdges => "tessellation.feature-edges",
            Self::TessellationCornerNormals => "tessellation.corner-normals",
            Self::TessellationTriangleGroups => "tessellation.triangle-groups",
            Self::TessellationTextureAssignments => "tessellation.texture-assignments",
            Self::TessellationInvalidCardinality => "tessellation.invalid-cardinality",
            Self::TessellationBodyLinkUnwritable => "tessellation.body-link-unwritable",
            Self::TessellationMetadataReduced => "tessellation.metadata-reduced",
            Self::TopologyUnreachableFromRegion => "topology.unreachable-from-region",
            Self::AnalyticSurfaceNormalized => "geometry.analytic-surface-normalized",
            Self::EllipticalConeReduced => "geometry.elliptical-cone-reduced",
            Self::CurvelessEdgeOmitted => "geometry.curveless-edge-omitted",
            Self::UnknownSurfaceFaceOmitted => "geometry.unknown-surface-face-omitted",
            Self::GeometryCarrierNotWritten => "geometry.carrier-not-written",
            Self::PcurveCarrierUnwritable => "pcurve.carrier-unwritable",
            Self::AssemblyOccurrenceOmittedNoParentProduct => {
                "assembly.occurrence-omitted-no-parent-product"
            }
            Self::RegionNoShellList => "topology.region-no-shell-list",
            Self::WireRegionNoConnectedEdgeSet => "topology.wire-region-no-connected-edge-set",
            Self::WireRegionMissingShell => "topology.wire-region-missing-shell",
            Self::HiddenBodyOmitted => "body.hidden-omitted",
            Self::HiddenPresentationLayerOmitted => "presentation.hidden-layer-omitted",
            Self::AppearanceBindingMissingAsset => "appearance.binding-missing-asset",
            Self::AppearanceBindingNoBaseColor => "appearance.binding-no-base-color",
            Self::AppearanceBindingTargetConflict => "appearance.binding-target-conflict",
            Self::CoedgePcurveNoGeometry => "pcurve.coedge-no-geometry",
            Self::CoedgePcurveNativeMetadata => "pcurve.coedge-native-metadata",
            Self::PcurveUseNativeMetadata => "pcurve.use-native-metadata",
            Self::CoedgeUseCurveNotRepresented => "topology.coedge-use-curve-not-represented",
            Self::TopologyMetadataNotRepresented => "topology.metadata-not-represented",
            Self::SubdOmitted => "geometry.subd-omitted",
            Self::ParametricDesignRecordsOmitted => "design.parametric-records-omitted",
            Self::DrawingPresentationRecordsOmitted => "presentation.drawing-records-omitted",
            Self::SemanticAnnotationOmitted => "pmi.semantic-annotation-omitted",
            Self::DocumentAssetOmitted => "asset.document-omitted",
            Self::AssemblyJointOmitted => "assembly.joint-omitted",
            Self::ProductNonPartKind => "product.non-part-kind",
            Self::ProductBomPropertyOmitted => "product.bom-property-omitted",
            Self::OccurrenceExternalProduct => "occurrence.external-product",
            Self::OccurrenceNoWritableProduct => "occurrence.no-writable-product",
            Self::OccurrenceMetadataOmitted => "occurrence.metadata-omitted",
            Self::PmiAnnotationNotWritten => "pmi.annotation-not-written",
            Self::SourceAssociationOmitted => "source.association-omitted",
            Self::PassthroughRecordOmitted => "native.passthrough-record-omitted",
            Self::DisplayColorUnstyled => "appearance.display-color-unstyled",
            Self::WireBodyTransparencyOmitted => "appearance.wire-body-transparency-omitted",
            Self::AppearanceReducedToBaseColor => "appearance.reduced-to-base-color",
            Self::AppearanceBindingMetadataReduced => "appearance.binding-metadata-reduced",
            Self::SourceAttributeNotWritten => "attribute.source-record-not-written",
            Self::ProceduralReducedToCarrier => "geometry.procedural-reduced-to-carrier",
            Self::ProceduralDefinitionNotWritten => "geometry.procedural-definition-not-written",
            Self::SourceNativeRecordOmitted => "native.source-record-omitted",
            Self::RegionNoWritableOuterShell => "topology.region-no-writable-outer-shell",
            Self::RegionOmittedVoidShell => "topology.region-omitted-void-shell",
            Self::ShellOmittedOuterFace => "topology.shell-omitted-outer-face",
            Self::ShellOmittedUnclassifiedFace => "topology.shell-omitted-unclassified-face",
            Self::FaceOmittedOuterLoop => "topology.face-omitted-outer-loop",
            Self::FaceOmittedInnerLoop => "topology.face-omitted-inner-loop",
            Self::FaceOmittedUnclassifiedLoop => "topology.face-omitted-unclassified-loop",
            Self::FaceNoWritableBounds => "topology.face-no-writable-bounds",
            Self::LoopRecordMissing => "topology.loop-record-missing",
            Self::LoopVertexMissing => "topology.loop-vertex-missing",
            Self::LoopCoedgeRecordMissing => "topology.loop-coedge-record-missing",
            Self::LoopEdgeNotWritable => "topology.loop-edge-not-writable",
            Self::LoopNoWritableEdges => "topology.loop-no-writable-edges",
            Self::LoopDuplicateCoedge => "topology.loop-duplicate-coedge",
            Self::LoopCoedgeMissingForOrder => "topology.loop-coedge-missing-for-order",
            Self::LoopEdgeMissingForOrder => "topology.loop-edge-missing-for-order",
            Self::EdgeNo3dCurve => "topology.edge-no-3d-curve",
            Self::EdgeCurveNotWritable => "topology.edge-curve-not-writable",
            Self::EdgeCurveUnsupported => "topology.edge-curve-unsupported",
            Self::LoopNoContinuousOrdering => "topology.loop-no-continuous-ordering",
            Self::EdgeRecordMissing => "topology.edge-record-missing",
            Self::EdgeStartVertexMissing => "topology.edge-start-vertex-missing",
            Self::EdgeEndVertexMissing => "topology.edge-end-vertex-missing",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ByteAccountingUnclassified
            | Self::DocumentLengthUnitUnresolved
            | Self::DocumentAngleUnitUnresolved
            | Self::LineParameterScaleUnresolved
            | Self::TopologyRootRejected
            | Self::TopologyRootIncomplete
            | Self::PcurveEndpointsDiscontinuous
            | Self::PcurveLocusDiscontinuous
            | Self::NauoPlacementUnresolved
            | Self::NauoPlacementAmbiguous
            | Self::BodyConflictingMappedPlacements
            | Self::PmiLengthUnitUnresolved
            | Self::PmiAngleUnitUnresolved
            | Self::ValidationMeasureUnitUnresolved
            | Self::RegionNoShellList
            | Self::RegionNoWritableOuterShell
            | Self::RegionOmittedVoidShell
            | Self::ShellOmittedOuterFace
            | Self::ShellOmittedUnclassifiedFace
            | Self::FaceOmittedOuterLoop
            | Self::FaceOmittedUnclassifiedLoop
            | Self::FaceNoWritableBounds
            | Self::LoopCoedgeRecordMissing
            | Self::LoopEdgeNotWritable
            | Self::LoopDuplicateCoedge
            | Self::LoopCoedgeMissingForOrder
            | Self::LoopEdgeMissingForOrder
            | Self::LoopNoContinuousOrdering => Severity::Error,
            Self::TessellationMetadataReduced
            | Self::CoedgePcurveNativeMetadata
            | Self::PcurveUseNativeMetadata
            | Self::CoedgeUseCurveNotRepresented
            | Self::TopologyMetadataNotRepresented
            | Self::ParametricDesignRecordsOmitted
            | Self::DrawingPresentationRecordsOmitted
            | Self::DocumentAssetOmitted
            | Self::AssemblyJointOmitted
            | Self::ProductNonPartKind
            | Self::ProductBomPropertyOmitted
            | Self::OccurrenceMetadataOmitted
            | Self::SourceAssociationOmitted
            | Self::PassthroughRecordOmitted
            | Self::DisplayColorUnstyled
            | Self::WireBodyTransparencyOmitted
            | Self::AppearanceReducedToBaseColor
            | Self::AppearanceBindingMetadataReduced
            | Self::SourceAttributeNotWritten
            | Self::ProceduralReducedToCarrier
            | Self::SourceNativeRecordOmitted => Severity::Info,
            _ => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerMemberNoncanonical
            | Self::ParseNoncanonicalSyntax
            | Self::ParsePreambleOmitted
            | Self::ParseEnvelopeIncomplete
            | Self::ParseRecordOmitted
            | Self::OrientedShellOmitsCfsFaces
            | Self::HeaderMetadataNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::DecodeWarning
            | Self::ByteAccountingUnclassified
            | Self::PcurveGlobalFidelityUnproved => LossTaxonomy::DecodeDiagnostic,
            Self::ParseDraftGrammar => LossTaxonomy::SourceDialectUnverified,
            Self::OpaqueRecordPreserved
            | Self::DrawingRecordTooFewParameters
            | Self::DrawingOrderUnstatable => LossTaxonomy::RecordNotTyped,
            Self::ImplementationLevelUnverified | Self::SourceDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::MetadataStringInvalid
            | Self::SchemaObjectIdentifierOutOfRange
            | Self::PresentationAnnotationTextUnordered
            | Self::PresentationAnnotationPlacementAmbiguous
            | Self::DimensionalNominalAmbiguous
            | Self::DimensionalUnnamedMeasureAmbiguous
            | Self::ConflictingScalarColors
            | Self::SurfaceSideInvalid
            | Self::SurfaceTransparencyConflict
            | Self::ContextDependentStyleUnresolved
            | Self::DrawingRelationshipUntypedTarget
            | Self::DraughtingSemanticDefinitionUntyped
            | Self::DraughtingAssociatedItemUntyped
            | Self::ProductNonPartKind
            | Self::ProductBomPropertyOmitted
            | Self::OccurrenceNoWritableProduct
            | Self::OccurrenceMetadataOmitted => LossTaxonomy::MetadataNotTransferred,
            Self::AttributeStringInvalid
            | Self::LayerItemWithoutCarrier
            | Self::HiddenBodyVisibilityUnsupported
            | Self::HiddenAppearanceVisibilityUnsupported
            | Self::HiddenPresentationLayerVisibilityUnsupported
            | Self::HiddenPmiVisibilityUnsupported
            | Self::HiddenPresentationLayerOmitted
            | Self::TessellationFeatureEdges
            | Self::TessellationCornerNormals
            | Self::TessellationTriangleGroups
            | Self::TessellationTextureAssignments
            | Self::TessellationMetadataReduced
            | Self::CoedgeUseCurveNotRepresented
            | Self::TopologyMetadataNotRepresented
            | Self::DrawingPresentationRecordsOmitted
            | Self::DisplayColorUnstyled
            | Self::SourceAttributeNotWritten => LossTaxonomy::AttributesNotTransferred,
            Self::PlacementReferenceInferred => LossTaxonomy::CarrierAxisInferred,
            Self::ConflictingRepresentationUnits
            | Self::UncertaintyLengthAmbiguous
            | Self::UncertaintyLengthUnresolved
            | Self::DocumentLengthUnitUnresolved
            | Self::DocumentAngleUnitUnresolved
            | Self::LineParameterScaleUnresolved
            | Self::PmiLengthUnitUnresolved
            | Self::PmiAngleUnitUnresolved
            | Self::ValidationMeasureUnitUnresolved
            | Self::GeometryCarrierNotWritten
            | Self::ProceduralDefinitionNotWritten => LossTaxonomy::GeometryNotTransferred,
            Self::TopologyRootRejected
            | Self::TopologyRootIncomplete
            | Self::AssemblyGraphInvalid
            | Self::OccurrenceUnresolvedParent
            | Self::OccurrenceNoLocalProduct
            | Self::WireShellFreeVertices
            | Self::BodyWithoutProductRepresentation
            | Self::SurfaceShellWireTopologyOmitted
            | Self::TessellationBodyLinkUnwritable
            | Self::TopologyUnreachableFromRegion
            | Self::RegionNoShellList
            | Self::WireRegionNoConnectedEdgeSet
            | Self::WireRegionMissingShell
            | Self::RegionNoWritableOuterShell
            | Self::RegionOmittedVoidShell
            | Self::ShellOmittedOuterFace
            | Self::ShellOmittedUnclassifiedFace
            | Self::FaceOmittedOuterLoop
            | Self::FaceOmittedUnclassifiedLoop
            | Self::FaceOmittedInnerLoop
            | Self::FaceNoWritableBounds
            | Self::LoopRecordMissing
            | Self::LoopVertexMissing
            | Self::LoopCoedgeRecordMissing
            | Self::LoopEdgeNotWritable
            | Self::LoopNoWritableEdges
            | Self::LoopDuplicateCoedge
            | Self::LoopCoedgeMissingForOrder
            | Self::LoopEdgeMissingForOrder
            | Self::EdgeNo3dCurve
            | Self::EdgeCurveNotWritable
            | Self::EdgeCurveUnsupported
            | Self::LoopNoContinuousOrdering
            | Self::EdgeRecordMissing
            | Self::EdgeStartVertexMissing
            | Self::EdgeEndVertexMissing => LossTaxonomy::TopologyNotTransferred,
            Self::SeamEdgePcurveUnresolved
            | Self::EdgeNoSurfaceOrCurveForPcurve
            | Self::PcurveAssociationAmbiguous
            | Self::PcurveCandidatesCarrierUnresolved
            | Self::DrawingRelationshipTargetAmbiguous
            | Self::DrawingSheetRevisionUnresolved
            | Self::DrawingRevisionSheetUnresolved
            | Self::TessellationItemBodyUnresolved
            | Self::TessellationItemUndeclared
            | Self::TessellationPlacementUnresolved
            | Self::TessellationPlacementAmbiguous => LossTaxonomy::ReferenceGraphNotClosed,
            Self::PcurveEndpointsDiscontinuous
            | Self::PcurveLocusDiscontinuous
            | Self::PcurveCarrierUnwritable
            | Self::CoedgePcurveNoGeometry
            | Self::CoedgePcurveNativeMetadata
            | Self::PcurveUseNativeMetadata => LossTaxonomy::PcurveOmitted,
            Self::FaceMultipleOuterBounds | Self::ShellDisconnectedFaces => {
                LossTaxonomy::SourceTopologyInvalid
            }
            Self::NauoPlacementUnresolved
            | Self::NauoPlacementAmbiguous
            | Self::BodyConflictingMappedPlacements
            | Self::AssemblyOccurrenceOmittedNoParentProduct
            | Self::AssemblyJointOmitted => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::NoExportableSolids => LossTaxonomy::NoExportableSolids,
            Self::RootOccurrencePlacementNotRepresentable
            | Self::OccurrencePlacementNotRigid
            | Self::BodyNonRigidTransform => LossTaxonomy::BodyTransformNotApplied,
            Self::TessellationRequiresAp242
            | Self::TessellationInvalidCardinality
            | Self::TessellationInvalidPayload => LossTaxonomy::TessellationOmitted,
            Self::AnalyticSurfaceNormalized => LossTaxonomy::AnalyticSurfaceNormalized,
            Self::EllipticalConeReduced => LossTaxonomy::EllipticalConeReduced,
            Self::CurvelessEdgeOmitted => LossTaxonomy::CurvelessEdgeOmitted,
            Self::UnknownSurfaceFaceOmitted => LossTaxonomy::UnknownSurfaceFaceOmitted,
            Self::HiddenBodyOmitted => LossTaxonomy::HiddenBodyOmitted,
            Self::AppearanceBindingMissingAsset
            | Self::AppearanceBindingNoBaseColor
            | Self::AppearanceBindingTargetConflict
            | Self::WireBodyTransparencyOmitted => LossTaxonomy::MaterialNotTransferred,
            Self::SubdOmitted => LossTaxonomy::SubdOmitted,
            Self::ParametricDesignRecordsOmitted | Self::SourceNativeRecordOmitted => {
                LossTaxonomy::ParametricRecordOmitted
            }
            Self::SemanticAnnotationOmitted
            | Self::PmiAnnotationNotWritten
            | Self::PmiDatumSystemInvalid => LossTaxonomy::PmiOmitted,
            Self::DocumentAssetOmitted => LossTaxonomy::AssetNotTransferred,
            Self::OccurrenceExternalProduct => LossTaxonomy::AssemblyComponentsExternal,
            Self::SourceAssociationOmitted => LossTaxonomy::SourceAssociationOmitted,
            Self::PassthroughRecordOmitted => LossTaxonomy::PassthroughRecordOmitted,
            Self::AppearanceReducedToBaseColor | Self::AppearanceBindingMetadataReduced => {
                LossTaxonomy::AppearanceReduced
            }
            Self::ProceduralReducedToCarrier => LossTaxonomy::ProceduralReduced,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `step/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::StepLossCode;
    use cadmpeg_ir::report::{loss::StrictConsequence, Severity};
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = StepLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "parse.noncanonical-syntax",
                "parse.record-omitted",
                "parse.preamble-omitted",
                "parse.draft-grammar",
                "parse.envelope-incomplete",
                "container.member-noncanonical",
                "decode.warning",
                "decode.byte-accounting-unclassified",
                "decode.opaque-record-preserved",
                "metadata.string-invalid",
                "metadata.schema-object-identifier-out-of-range",
                "parse.implementation-level-unverified",
                "parse.header-metadata-noncanonical",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "attribute.string-invalid",
                "geometry.placement-reference-inferred",
                "geometry.conflicting-representation-units",
                "geometry.uncertainty-length-ambiguous",
                "geometry.uncertainty-length-unresolved",
                "geometry.document-length-unit-unresolved",
                "geometry.document-angle-unit-unresolved",
                "geometry.line-parameter-scale-unresolved",
                "topology.root-rejected",
                "topology.root-incomplete",
                "topology.oriented-shell-omits-cfs-faces",
                "topology.seam-edge-pcurve-unresolved",
                "topology.edge-no-surface-or-curve-for-pcurve",
                "topology.pcurve-endpoints-discontinuous",
                "topology.pcurve-locus-discontinuous",
                "topology.pcurve-global-fidelity-unproved",
                "topology.pcurve-association-ambiguous",
                "topology.pcurve-candidates-carrier-unresolved",
                "topology.face-multiple-outer-bounds",
                "topology.shell-disconnected-faces",
                "product.nauo-placement-unresolved",
                "product.nauo-placement-ambiguous",
                "product.body-conflicting-mapped-placements",
                "pmi.presentation-annotation-text-unordered",
                "pmi.presentation-annotation-placement-ambiguous",
                "pmi.dimensional-nominal-ambiguous",
                "pmi.dimensional-unnamed-measure-ambiguous",
                "pmi.length-unit-unresolved",
                "pmi.angle-unit-unresolved",
                "pmi.datum-system-invalid",
                "presentation.conflicting-scalar-colors",
                "presentation.surface-side-invalid",
                "presentation.surface-transparency-conflict",
                "presentation.context-dependent-style-unresolved",
                "drawing.record-too-few-parameters",
                "drawing.order-unstatable",
                "drawing.relationship-untyped-target",
                "drawing.relationship-target-ambiguous",
                "drawing.sheet-revision-unresolved",
                "drawing.revision-sheet-unresolved",
                "drawing.draughting-semantic-definition-untyped",
                "drawing.draughting-associated-item-untyped",
                "tessellation.item-body-unresolved",
                "tessellation.invalid-payload",
                "tessellation.item-undeclared",
                "tessellation.placement-unresolved",
                "tessellation.placement-ambiguous",
                "validation.measure-unit-unresolved",
                "export.no-exportable-solids",
                "layer.item-without-carrier",
                "assembly.graph-invalid",
                "occurrence.root-placement-not-representable",
                "occurrence.unresolved-parent",
                "occurrence.no-local-product",
                "occurrence.placement-not-rigid",
                "body.non-rigid-transform",
                "body.hidden-visibility-unsupported",
                "presentation.hidden-appearance-visibility-unsupported",
                "presentation.hidden-layer-visibility-unsupported",
                "pmi.hidden-visibility-unsupported",
                "topology.wire-shell-free-vertices",
                "product.body-without-representation",
                "topology.surface-shell-wire-omitted",
                "tessellation.requires-ap242",
                "tessellation.feature-edges",
                "tessellation.corner-normals",
                "tessellation.triangle-groups",
                "tessellation.texture-assignments",
                "tessellation.invalid-cardinality",
                "tessellation.body-link-unwritable",
                "tessellation.metadata-reduced",
                "topology.unreachable-from-region",
                "geometry.analytic-surface-normalized",
                "geometry.elliptical-cone-reduced",
                "geometry.curveless-edge-omitted",
                "geometry.unknown-surface-face-omitted",
                "geometry.carrier-not-written",
                "pcurve.carrier-unwritable",
                "assembly.occurrence-omitted-no-parent-product",
                "topology.region-no-shell-list",
                "topology.wire-region-no-connected-edge-set",
                "topology.wire-region-missing-shell",
                "body.hidden-omitted",
                "presentation.hidden-layer-omitted",
                "appearance.binding-missing-asset",
                "appearance.binding-no-base-color",
                "appearance.binding-target-conflict",
                "pcurve.coedge-no-geometry",
                "pcurve.coedge-native-metadata",
                "pcurve.use-native-metadata",
                "topology.coedge-use-curve-not-represented",
                "topology.metadata-not-represented",
                "geometry.subd-omitted",
                "design.parametric-records-omitted",
                "presentation.drawing-records-omitted",
                "pmi.semantic-annotation-omitted",
                "asset.document-omitted",
                "assembly.joint-omitted",
                "product.non-part-kind",
                "product.bom-property-omitted",
                "occurrence.external-product",
                "occurrence.no-writable-product",
                "occurrence.metadata-omitted",
                "pmi.annotation-not-written",
                "source.association-omitted",
                "native.passthrough-record-omitted",
                "appearance.display-color-unstyled",
                "appearance.wire-body-transparency-omitted",
                "appearance.reduced-to-base-color",
                "appearance.binding-metadata-reduced",
                "attribute.source-record-not-written",
                "geometry.procedural-reduced-to-carrier",
                "geometry.procedural-definition-not-written",
                "native.source-record-omitted",
                "topology.region-no-writable-outer-shell",
                "topology.region-omitted-void-shell",
                "topology.shell-omitted-outer-face",
                "topology.shell-omitted-unclassified-face",
                "topology.face-omitted-outer-loop",
                "topology.face-omitted-inner-loop",
                "topology.face-omitted-unclassified-loop",
                "topology.face-no-writable-bounds",
                "topology.loop-record-missing",
                "topology.loop-vertex-missing",
                "topology.loop-coedge-record-missing",
                "topology.loop-edge-not-writable",
                "topology.loop-no-writable-edges",
                "topology.loop-duplicate-coedge",
                "topology.loop-coedge-missing-for-order",
                "topology.loop-edge-missing-for-order",
                "topology.edge-no-3d-curve",
                "topology.edge-curve-not-writable",
                "topology.edge-curve-unsupported",
                "topology.loop-no-continuous-ordering",
                "topology.edge-record-missing",
                "topology.edge-start-vertex-missing",
                "topology.edge-end-vertex-missing",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in StepLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in StepLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    /// The admission warning reports transferred source data and its
    /// verification status, so strict decode keeps it.
    #[test]
    fn globally_unproved_pcurve_admission_does_not_refuse_strict_decode() {
        let note = StepLossCode::PcurveGlobalFidelityUnproved.note("x");
        assert_eq!(note.severity, Severity::Warning);
        assert_eq!(note.strict_consequence(), StrictConsequence::Tolerate);
    }

    /// A kept default replaces a source value, so strict decode refuses it.
    #[test]
    fn substituted_length_uncertainty_refuses_strict_decode() {
        let note = StepLossCode::UncertaintyLengthAmbiguous.note("x");
        assert_eq!(note.severity, Severity::Warning);
        assert_eq!(note.strict_consequence(), StrictConsequence::Reject);
    }
}
