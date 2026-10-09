# Sheet metal

The SHEET METAL tab builds parts from a sheet of constant thickness under a **sheet metal rule**
(thickness, bend radius, K-factor), set in **Sheet Metal Rules** (`sheet.manage_rules`).

- **Flange** (`sheet.flange`): a base flange from a sketch profile, a contour flange from an open
  sketch, or an edge flange from a part edge.
- **Hem** (`sheet.hem`) and **Fold** (`sheet.fold`) along a sketch line.
- **Unfold / Refold** (`sheet.unfold`, `sheet.refold`) for working on the flat part.
- **Convert** (`sheet.convert`) turns a solid of constant thickness into sheet metal.
- **Create Flat Pattern** (`sheet.flat_pattern`) shows the flat part with its bend lines and a
  bend table, and exports it as DXF.

Bends follow the rule's K-factor, so flat lengths match what a press brake will make. The
oracle checks these against parts built in Fusion.
