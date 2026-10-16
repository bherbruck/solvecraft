# Fusion offline corpus smoke test

Desktop F3D import is exercised file-by-file with:

```bash
cargo xtask f3d-corpus /path/to/f3d/folder --timeout 300
```

Each `.f3d` runs `solidcraft-io` test `tests::fusion_open_one_file` in its own process so one heavy file cannot hang the whole run. Lines look like:

- `RESULT OK solid … bodies=N`
- `RESULT OK mesh … meshes=N`
- `RESULT FAIL …`
- `RESULT TIMEOUT …`

Set `F3D_CORPUS_STRICT=1` when running `cargo test -p solidcraft-io fusion_offline_corpus_smoke` to fail CI if any file in the directory fails.
