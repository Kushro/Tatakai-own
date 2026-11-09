# tk-core - dependency notes (B5)

No new workspace dependency is required. Crate-level additions use dependencies that already exist in `[workspace.dependencies]`:
`bytes` (tk-core::net), `base64`/`hex`/`uuid`/`futures-util` (tk-ext).

`time` is used WITHOUT the `macros` feature (tests build instants with `from_unix_timestamp`).

## Wave B, worker h (download engine): AES-128 HLS segments

`src/download/hls.rs::decrypt_aes128` decrypts `#EXT-X-KEY:METHOD=AES-128` segments (AES-128-CBC, PKCS#7) with the
RustCrypto crates. Both are **already in `Cargo.lock`** (transitive, same versions), pure Rust, no CMake / NASM /
OpenSSL, MIT OR Apache-2.0:

| Crate | Version | Features | Add to |
|---|---|---|---|
| `aes` | `0.8.4` (`aes = "0.8"`) | defaults | `[workspace.dependencies]` + `tk-core/[dependencies]` (`aes = { workspace = true }`) |
| `cbc` | `0.1.2` (`cbc = "0.1"`) | defaults (`block-padding`) | `[workspace.dependencies]` + `tk-core/[dependencies]` (`cbc = { workspace = true }`) |

Used API: `cbc::Decryptor::<aes::Aes128>::new(&key.into(), &iv.into()).decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)`
with `aes::cipher::{BlockDecryptMut, KeyIvInit}` (the `cbc` 0.1 crate docs). The test `hls::tests::aes_cbc_vector` uses
the vector from those docs (`hex` is already a tk-core dependency).

`tokio::fs`, `tokio::process`-free `std::process::Command` (optional `ffmpeg` remux) and `futures-util` /
`m3u8-rs` / `notify` (already listed by the contract) cover the rest.
