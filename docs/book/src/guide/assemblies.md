# Components and joints

A design is a tree of **components**. **New Component** (`component.create`) adds one, and
**Create Components from Bodies** (`component.from_bodies`) turns bodies into components. Each
component has its own origin, bodies, sketches and timeline entries. The **active component**
(the radio button in the browser) receives new sketches and features; the others are drawn
faded.

A component is placed by its **occurrences**. Paste creates another instance of the same
component, and **Paste New** creates an independent copy. Move an occurrence with the move
triad, then **Capture Position** to keep it or **Revert** to put it back. SolveCraft asks before
the next command if a move wasn't captured. **Ground** fixes an occurrence in place.

**Joints** (`joint.create`, J) connect two components by joint origins that snap to faces, edges,
circles and vertices: rigid, revolute, slider, cylindrical, pin-slot, planar and ball, with
offsets, flip and limits. **As-built Joint** keeps the current position, **Rigid Group** locks
components together, **Drive Joints** moves a joint through its values, and **Motion Link**
couples two joints. Contact sets stop motion where bodies would pass through each other,
**Motion Study** keys joint values along a timeline, and **Interference** shows overlapping
volumes in red.

STEP export writes the component tree as an assembly.
