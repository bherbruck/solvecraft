# Import and export

| Format | Read | Write |
|---|---|---|
| `.solvecraft` (JSON design, versioned) | yes | yes |
| STEP (AP203, AP214, AP242) | solids, assemblies, units, names, colours | AP242 with names, colours and the component tree |
| IGES 5.3 | manifold solids, or trimmed surfaces sewn into solids | manifold solids with names and colours |
| 3MF, STL, OBJ | mesh bodies | yes |
| DXF, SVG | into a sketch | sketches and flat patterns as DXF |
| PNG, JPEG | canvases and decals | |

Opening a STEP or IGES file creates a design with the file's bodies as a base feature, and
inserting one adds them to the current design. Later features build on them. Mesh bodies render,
measure, move and export, but solid features need B-rep bodies.

Saves are atomic: a crash at any moment leaves either the old file or the new one, and the
replaced version is kept as `<file>.bak`. Unsaved changes are autosaved to a recovery folder,
and the next launch offers to recover them. Older design files are upgraded on open, and files
from a newer version are refused with a message.
