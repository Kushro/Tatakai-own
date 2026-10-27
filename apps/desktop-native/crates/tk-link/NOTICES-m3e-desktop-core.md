# Third-party / own-repo origins (branch m3e/desktop-core: tk-core, tk-ext, tk-link)

To be consolidated into `apps/desktop-native/THIRD_PARTY_NOTICES.md` by the integrator.

| Code | Origin | License | Notes |
|---|---|---|---|
| `tk-link/src/identity.rs` (CA, server certificate, client certificate issuance, SAN renewal) | Enigmata-Temple `crates/temple-core/src/identity.rs` | AGPL-3.0-or-later (own repo of the user) | aws-lc-rs replaced by ring/uuid randomness; keys protected with DPAPI; CSR handled with rcgen `CertificateSigningRequestParams` |
| `tk-link/src/pairing.rs` (Crockford code, window state machine, device-name sanitising, payload) | Enigmata-Temple `temple-core/src/pairing.rs` | AGPL-3.0-or-later | payload prefix `tatakai-link:v1`; constant-time compare hand-written |
| `tk-link/src/media/anime4k.rs` (pass chains per mode, `.hook` composition) | Enigmata-Temple `temple-core/src/media/anime4k.rs` | AGPL-3.0-or-later; shaders are Anime4K v4.0 by bloc97 (MIT, AutoDownscalePre: Unlicense) | shaders are NOT embedded; downloaded by `packaging/fetch-deps.ps1` into `LinkConfig::anime4k_dir` and verified against `packaging/anime4k.lock.json` |
| `tk-link/src/media/args.rs` (graph shape: d3d11va decode, libplacebo + custom_shader_path, frc_amf retention compensation) | Enigmata-Temple `temple-core/src/media/args.rs` | AGPL-3.0-or-later | simplified; argument vectors are golden-tested |
| `tk-link/src/devices.rs`, `tk-link/src/server.rs` structure | Enigmata-Temple `devices.rs` / `server.rs` | AGPL-3.0-or-later | rewritten for axum 0.8 + hyper-util without the `tower` crate |
| `tk-ext/src/js/prelude.js` shim semantics | `src/core/extensions/mobile/{requireShim,parseHtml}.ts` of this repository (MPL-2.0) | MPL-2.0 | re-implemented in JS for QuickJS |
| metadata router semantics | `src/lib/metadata/router.ts` of this repository | MPL-2.0 | |
