//! tk-ui: Dioxus 0.7 UI for Tatakai Next (router, shell, screens, components, i18n en/es).
//!
//! The crate never touches tk-core / tk-ext / tk-link directly: tk-app implements [`backend::Backend`]
//! over them. Pure state lives in [`model`] / [`shortcuts`] and is unit-tested without Dioxus.

pub mod app;
pub mod backend;
pub mod components;
pub mod fonts;
pub mod i18n;
pub mod icons;
pub mod model;
pub mod platform;
pub mod qr;
pub mod screens;
pub mod shortcuts;

pub use app::{App, Services};
pub use backend::{Backend, BackendRef};
