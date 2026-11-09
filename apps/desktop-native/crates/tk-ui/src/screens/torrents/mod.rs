//! Torrent client (docs/redesign/10 section 5): `layout::TorrentsLayout` (side index like Settings + `Outlet`) and one
//! page per `model::TorrentSection`. Owners: wave 3 C (`layout`, `overview`, `files`, `peers`, `trackers`), wave 3 D
//! (`library`, `downloads`, `settings`). Each owner may add private files here (registered with one additive line).
//!
//! Wave A (docs/redesign/15): worker **i** owns every file here plus `debrid` (debrid services as sources) and
//! `history` (torrent session history, plan 14 section 0.1 #11).
//!
//! Shared state: `app::Ui::torrent` (selected torrent id) for the per-torrent pages. Data: `Backend::torrent_*`.

pub mod debrid;
pub mod downloads;
pub mod files;
pub mod history;
pub mod layout;
pub mod library;
pub mod overview;
pub mod peers;
pub mod settings;
pub mod trackers;
