//! `tatakai-next`: wires tk-ui + tk-core + tk-ext + tk-link + tk-player into one Dioxus desktop window.
//!
//! Window: frameless and TRANSPARENT (review E5). The web view is transparent too; the page paints its own
//! background and the player screen leaves a hole through which mpv's child window shows.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod backend_common;
mod config;
mod firewall;
#[cfg(feature = "live")]
mod live;
mod logging;
#[cfg(not(feature = "live"))]
mod offline;
mod paths;
mod settings_store;
mod single_instance;

use std::sync::Arc;

use dioxus::LaunchBuilder;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use tk_player::PlayerHost;
use tk_ui::{BackendRef, Services};

const APP_ID: &str = "app.tatakai.own.next";
/// Also the title `single_instance` searches for.
const WINDOW_TITLE: &str = "Tatakai";

#[cfg(feature = "live")]
fn build_backend(paths: &paths::AppPaths, cfg: &config::AppConfig) -> anyhow::Result<BackendRef> {
    Ok(Arc::new(live::LiveBackend::new(paths, cfg)?))
}

#[cfg(not(feature = "live"))]
fn build_backend(paths: &paths::AppPaths, _cfg: &config::AppConfig) -> anyhow::Result<BackendRef> {
    tracing::warn!("built without the `live` feature: catalogue, extensions and Link are not wired");
    Ok(Arc::new(offline::OfflineBackend::new(paths.ui_settings_file())))
}

/// The window configuration (E5): transparent window + transparent web view background.
fn window_config(paths: &paths::AppPaths) -> Config {
    let window = WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0));
    Config::new()
        .with_window(window)
        .with_background_color((0, 0, 0, 0))
        .with_menu(None)
        .with_disable_context_menu(true)
        .with_data_directory(paths.webview_dir())
}

fn main() -> anyhow::Result<()> {
    let paths = paths::AppPaths::resolve()?;
    paths.ensure()?;
    if let Err(e) = logging::init(&paths.logs_dir()) {
        eprintln!("logging disabled: {e}");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %paths.root.display(), "starting tatakai-next");

    // Keep the guard alive for the whole run.
    let Some(_instance) = single_instance::acquire(APP_ID, WINDOW_TITLE) else {
        tracing::info!("another instance is running; asked it to show its window");
        return Ok(());
    };

    let cfg = config::AppConfig::load_or_create(&paths.root, config::AppConfig::from_build_env())?;
    let backend = build_backend(&paths, &cfg)?;
    let services = Services {
        backend,
        player: Arc::new(PlayerHost::new(paths::shaders_dir(&paths, None))),
        fonts_dir: paths::fonts_dir(None),
    };

    LaunchBuilder::desktop().with_cfg(window_config(&paths)).with_context(services).launch(tk_ui::App);
    Ok(())
}
