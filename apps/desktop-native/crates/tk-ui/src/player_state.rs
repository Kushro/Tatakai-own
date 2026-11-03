//! Player flow state (wave 1 E, docs/redesign/08): Resolving / Buffering / Playing / Error / Manual, source
//! failover over the ranked `SourceVm` list and the `failed[]` record. Pure and unit-tested like `model`;
//! no Dioxus, no backend, no clock (callers pass `now`). Reserved here so E never edits `lib.rs`.
