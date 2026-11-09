//! `tatakai-next`: wires tk-ui + tk-core + tk-ext + tk-link + tk-player into one Dioxus desktop window.
//!
//! Window: frameless and TRANSPARENT (review E5). The web view is transparent too; the page paints its own
//! background and the player screen leaves a hole through which mpv's child window shows.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod anime_build;
// wave A (docs/redesign/15 section 2): autostart (f), discord (e), update_check (f); the tray lives in tk-ui.
#[allow(dead_code)] // used by worker f's live/account.rs
mod autostart;
mod backend_common;
mod config;
#[allow(dead_code)] // used by worker e's live/presence.rs
mod discord;
mod firewall;
#[cfg(feature = "live")]
mod live;
mod logging;
#[cfg(not(feature = "live"))]
mod offline;
mod paths;
mod settings_store;
mod single_instance;
#[allow(dead_code)] // used by worker f's live/account.rs
mod update_check;

use std::sync::Arc;

use dioxus::LaunchBuilder;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use tk_player::PlayerHost;
use tk_ui::ui_thread::{self, UiThread};
use tk_ui::{BackendRef, Services};

const APP_ID: &str = "app.tatakai.own.next";
/// Also the title `single_instance` searches for.
const WINDOW_TITLE: &str = "Tatakai";

/// Start-up job run by `tk_ui::App` on its first render, inside the Dioxus runtime (`Services::on_start`).
type OnStart = Option<Arc<dyn Fn() + Send + Sync>>;

#[cfg(feature = "live")]
fn build_backend(paths: &paths::AppPaths, cfg: &config::AppConfig, ui: UiThread) -> anyhow::Result<(BackendRef, OnStart)> {
    let backend = Arc::new(live::LiveBackend::new(paths, cfg, ui)?);
    // Wave A, worker f: restore the persisted Supabase session once the UI (and its async runtime) is up.
    let on_start: Arc<dyn Fn() + Send + Sync> = {
        let backend = backend.clone();
        Arc::new(move || {
            let backend = backend.clone();
            dioxus::prelude::spawn(async move { backend.restore_session().await });
        })
    };
    Ok((backend, Some(on_start)))
}

#[cfg(not(feature = "live"))]
fn build_backend(paths: &paths::AppPaths, _cfg: &config::AppConfig, _ui: UiThread) -> anyhow::Result<(BackendRef, OnStart)> {
    tracing::warn!("built without the `live` feature: catalogue, extensions and Link are not wired");
    Ok((Arc::new(offline::OfflineBackend::new(paths.ui_settings_file())), None))
}

/// The window configuration (E5): transparent window + transparent web view background.
/// Only Windows needs the transparency (mpv's child HWND shows through the page); WebKitGTK on Wayland
/// flickers with a transparent toplevel, so elsewhere the window is opaque.
fn window_config(paths: &paths::AppPaths) -> Config {
    let transparent = cfg!(windows);
    let window = WindowBuilder::new()
        .with_title(WINDOW_TITLE)
        .with_decorations(false)
        .with_transparent(transparent)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0));
    Config::new()
        .with_window(window)
        .with_background_color(if transparent { (0, 0, 0, 0) } else { (0, 0, 0, 255) })
        .with_menu(None)
        .with_disable_context_menu(true)
        .with_data_directory(paths.webview_dir())
        // Dioxus defaults to exporting WEBKIT_DISABLE_DMABUF_RENDERER=1 and GDK_BACKEND=x11 on Wayland. Without the
        // DMA-BUF renderer WebKitGTK stops compositing the page with alpha over the embedded video (GL area under
        // the web view): the picture stays black and moving page elements leave trails. The variables also leak
        // into every child process (external mpv). Users who need the workaround still set it themselves.
        .with_disable_dma_buf_on_wayland(false)
}

fn main() -> anyhow::Result<()> {
    // CI smoke check: print the version and exit before touching data dirs, the single-instance lock or any window.
    if std::env::args_os().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("tatakai-next {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
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
    let (ui, ui_jobs) = ui_thread::channel();
    let (backend, on_start) = build_backend(&paths, &cfg, ui)?;
    let services = Services {
        backend,
        player: Arc::new(PlayerHost::new(paths::shaders_dir(&paths, None))),
        fonts_dir: paths::fonts_dir(None),
        ui_jobs,
        on_start,
    };

    LaunchBuilder::desktop().with_cfg(window_config(&paths)).with_context(services).launch(tk_ui::App);
    Ok(())
}
