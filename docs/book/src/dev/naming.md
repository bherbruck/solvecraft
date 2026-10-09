# Persistent naming

A fillet stores the edges it rounds, and a sketch stores the face it sits on. If those were face
and edge *indices*, an upstream edit that adds a face would move the fillet to the wrong edge.
SolveCraft stores **names** instead (`crates/doc/src/naming.rs`):

- A face of an evaluated body is named after how it was made. A face lying on a face of the
  feature's input keeps that face's name; a face new to the feature is named by its role:
  `F<feature>:side:<sketch curve>`, `F<feature>:start` and `:end`, `F<feature>:blend:<k>` and
  so on.
- Pieces of a split face get `#1`, `#2`, … in order along the longest axis.
- An edge is named by the two faces it separates.
- Pattern and mirror copies are named after their sources plus the instance.

Names are computed lazily, the first time something asks, from the body, the feature that made
it and the model just before that feature. Face provenance from the kernel (which input face an
output face came from) does most of the work.

When a feature evaluates, each stored reference is resolved by name. If no face has the name
any more, the reference falls back to the geometry it recorded (a point on the face, a normal),
and the feature shows a warning so the user can check it. A reference that matches nothing is
an error, never a guess.
