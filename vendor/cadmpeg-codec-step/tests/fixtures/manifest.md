# Synthetic STEP fixture manifest

All fixtures in this directory are original minimal exchange structures
authored against `docs/formats/step.md`.

| File | Construct |
| --- | --- |
| `ap242_minimal.p21` | AP242 header, one DATA section, forward reference, multiline record, numeric forms, enum, omitted and derived values |
| `ap242_ed3_sections.p21` | edition-3 ANCHOR, REFERENCE, multiple DATA, and SIGNATURE sections |
| `complex_instance.p21` | external-mapped complex instance with sibling-supplied attributes |
| `strings.p21` | apostrophe, reverse-solidus, X, X2, X4, S, and page-selection string escapes |
| `ap242_geometry.p21` | millimetre unit context, analytic/polyline carriers, unknown-periodicity NURBS, parameter and Cartesian trims, standalone geometric-set representation, trimmed composite, direct/set/source geometry styles, and null/empty styles |
| `ap214_sheet.p21` | connected triangular sheet B-rep with oriented edge uses and face/edge styles |
| `ap203_sheet.p21` | CONFIG_CONTROL_DESIGN connected triangular sheet B-rep with millimetre units |
| `ap242_assembly.p21` | product-definition tree, NAUO identity, and item-defined occurrence placement |
| `ap242_tessellation.p21` | exact/tessellated body linkage, one-based indices, tessellation style, and area/volume/centroid validation properties |
| `ap242_semantic_pmi.p21` | datum, datum system, base and datum-feature dimensional sizes with numeric and limits-and-fits tolerances, and complex-instance geometric-tolerance magnitude |
| `ap242_conversion_units.p21` | conversion-based length unit chain and representation uncertainty |
| `ap242_presentation_pmi.p21` | drafting model, annotation plane, placed text literal, and annotation occurrence |
| `ap242_mapped_assembly.p21` | representation map and mapped-item occurrence placement |
| `ap242_external_documents.p21` | applied document reference and externally defined item dependency identities |
| `ap242_geometric_set.p21` | geometrically bounded surface representation with a curve-bounded sheet carrier |
| `ap242_degree_cone.p21` | apex-zero conical surface with a conversion-based degree context |
| `ap242_vertex_loop.p21` | spherical face with singleton vertex boundaries at surface singularities |
