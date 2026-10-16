# cadmpeg-core

`cadmpeg-core` supplies bounded byte decoding primitives and resource
policy shared by cadmpeg format codecs. Format crates depend on it for
bounded address-space views, little- and big-endian readers, decode arenas,
container summaries, and `CodecError`.

Application code usually depends on a format crate such as
`cadmpeg-codec-f3d` or `cadmpeg-ir`. This crate is for codec authors.

## Install

```sh
cargo add cadmpeg-core
```

## Documentation

- [API documentation][docs]
- [Architecture and crate map][architecture]
- [Clean-room and legal policy][legal]
- [Repository][repo]

Requires Rust 1.88 or later. Licensed under Apache-2.0.

[architecture]: https://github.com/cadmpeg/cadmpeg/blob/main/docs/architecture.md
[docs]: https://docs.rs/cadmpeg-core
[legal]: https://github.com/cadmpeg/cadmpeg/blob/main/LEGAL.md
[repo]: https://github.com/cadmpeg/cadmpeg
