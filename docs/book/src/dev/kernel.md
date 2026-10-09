# The kernel boundary and truck

`crates/kernel` is SolveCraft's B-rep kernel. It wraps
[truck](https://github.com/ricosjp/truck) (Apache-2.0) for topology, NURBS geometry, booleans,
tessellation and STEP parsing, and it is the only crate allowed to use truck. Callers get
SolveCraft types (`Body`, face and edge indices, meshes, measures) and a
`KernelError` instead of truck types and panics.

## `guard`

truck can panic on geometry it doesn't handle. Every call into it goes through
`solvecraft_kernel::guard`, which catches the panic and turns it into
`KernelError::Internal`, leaving the model unchanged:

```rust
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T>
```

A failing feature shows an error on the timeline; it never takes the app down.

## What is our own

truck has no fillets, chamfers, shells or face edits, so these are SolveCraft code in the
kernel: constant, chord and variable-radius fillets with corner blends, chamfers, shell and
offset, Delete Face, Split Face, Replace Face, sewing, STEP and IGES reading and writing beyond
truck's parser, and face provenance (which input face each output face came from, for
[persistent naming](naming.md)).

## Vendored truck crates

Patched copies of `truck-shapeops`, `truck-stepio` and `truck-meshalgo` live in `vendor/`,
wired in with `[patch.crates-io]`. The patches fix robustness bugs found by our tests and the
STEP corpus. `vendor/` is excluded from our fmt and clippy rules, and each patch should go
upstream when it can.

Some truck assertions only run in debug builds and can fail on valid geometry, so tests and CI
run with `--release`.
