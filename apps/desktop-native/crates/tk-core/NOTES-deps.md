# tk-core - dependency notes (B5)

No new workspace dependency is required. Crate-level additions use dependencies that already exist in `[workspace.dependencies]`:
`bytes` (tk-core::net), `base64`/`hex`/`uuid`/`futures-util` (tk-ext).

`time` is used WITHOUT the `macros` feature (tests build instants with `from_unix_timestamp`).
