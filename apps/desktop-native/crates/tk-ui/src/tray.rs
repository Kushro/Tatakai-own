//! Tray icon, close-to-tray, start minimized and the new-version notice (wave A, worker **f**;
//! docs/redesign/15-parity-wave-a-contract.md section 4, checklist `system.tray.icon`, `system.tray.restore`,
//! `system.update-startup-notice`; classic `desktop/window/tray.cjs` and `desktop/services/update-manager.cjs`).
//!
//! The tray lives in tk-ui because dioxus-desktop's tray support needs the window's event loop: the icon is built
//! with `dioxus::desktop::trayicon::TrayIconBuilder` (re-export of `tray-icon` 0.21; not `init_tray_icon`, which
//! panics when the platform has no tray), the menu with `trayicon::menu::{Menu, MenuItem}` (Show, Quit), clicks
//! arrive through `use_tray_menu_event_handler` / `use_tray_icon_event_handler`, and
//! `DesktopContext::set_close_behavior` makes the close button hide the window while `Settings::system`
//! asks for `tray_icon && close_to_tray`. The effect watches `Ui::settings` (and the language) so toggling the
//! setting creates / drops the icon and relabels the menu live.
//!
//! Linux: `tray-icon` loads `libayatana-appindicator3` / `libappindicator3` at run time and panics when neither
//! is installed; building runs under `catch_unwind` and [`available`] reports what happened, so Settings > System
//! hides the tray rows instead of offering a switch that does nothing.
//!
//! Start minimized: the OS autostart entry passes [`MINIMIZED_ARG`] (`tk-app/src/autostart.rs`); with
//! `Settings::system.start_minimized` and a working tray the window hides once, right after the saved settings load.
//!
//! New-version notice: once per launch, after the saved settings load and the first-run wizard is done, ask
//! `Backend::check_updates` (tk-app caches the answer of its own start-up job, `live/system.rs::on_start`) and,
//! when a newer release exists that the user did not skip (`SystemPrefs::should_notify`), show a snackbar with
//! "Open" (release page through `Backend::open_url`). "Skip this version" lives in Settings > About.

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use dioxus::desktop::trayicon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use dioxus::desktop::trayicon::{DioxusTrayIcon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use dioxus::desktop::{DesktopContext, WindowCloseBehaviour, icon_from_memory, use_tray_icon_event_handler, use_tray_menu_event_handler, use_window};
use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::components::use_snackbar;
use crate::i18n::{t, tf};

/// Tray / window icon (`assets/icon.png`, 64x64 RGBA, the app logo).
pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
/// Argument of the OS autostart entry (same value as `tk-app/src/autostart.rs::MINIMIZED_ARG`).
pub const MINIMIZED_ARG: &str = "--minimized";
/// Seconds after the settings load before the new-version snackbar (lets the first screen settle).
const NOTICE_DELAY_SECS: u64 = 4;
/// Shared libraries `tray-icon` needs on Linux (it tries them in this order).
const LINUX_INDICATOR_LIBS: [&str; 2] = ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];
/// Where distributions put them (plus `LD_LIBRARY_PATH`).
const LINUX_LIB_DIRS: [&str; 10] = [
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/lib",
    "/lib64",
    "/lib/x86_64-linux-gnu",
    "/lib/aarch64-linux-gnu",
    "/usr/local/lib",
    "/app/lib",
];

/// 0 = not tried yet, 1 = an icon was built in this process, 2 = building failed.
static TRAY_STATE: AtomicU8 = AtomicU8::new(0);

/// Whether this platform can show a tray icon (`SystemCapsVm::tray`): Windows and macOS always; Linux when an
/// AppIndicator library is installed (or an icon was already built in this process).
pub fn available() -> bool {
    match TRAY_STATE.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        return true;
    }
    if cfg!(target_os = "linux") {
        let mut dirs: Vec<PathBuf> =
            std::env::var_os("LD_LIBRARY_PATH").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
        dirs.extend(LINUX_LIB_DIRS.iter().map(PathBuf::from));
        return indicator_lib_in(&dirs);
    }
    false
}

/// One of [`LINUX_INDICATOR_LIBS`] exists in one of `dirs`. Pure apart from `exists`.
pub fn indicator_lib_in(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| LINUX_INDICATOR_LIBS.iter().any(|lib| Path::new(d).join(lib).exists()))
}

/// The process was started by the OS autostart entry (`--minimized`).
pub fn has_minimized_arg(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter().skip(1).any(|a| a == MINIMIZED_ARG)
}

/// The live icon and the two menu items (kept for their ids and to relabel them).
struct TrayState {
    _icon: TrayIcon,
    show: MenuItem,
    quit: MenuItem,
}

impl TrayState {
    /// Follow the current language.
    fn relabel(&self) {
        let (show, quit) = (t("tray.show"), t("tray.quit"));
        if self.show.text() != show {
            self.show.set_text(show);
        }
        if self.quit.text() != quit {
            self.quit.set_text(quit);
        }
    }
}

fn build_tray() -> Option<TrayState> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<TrayState, String> {
        let show = MenuItem::new(t("tray.show"), true, None);
        let quit = MenuItem::new(t("tray.quit"), true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let icon = icon_from_memory::<DioxusTrayIcon>(ICON_PNG).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("Tatakai")
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(TrayState { _icon: tray, show, quit })
    }));
    match attempt {
        Ok(Ok(state)) => {
            TRAY_STATE.store(1, Ordering::Relaxed);
            Some(state)
        }
        Ok(Err(e)) => {
            TRAY_STATE.store(2, Ordering::Relaxed);
            tracing::warn!(error = %e, "the tray icon could not be created");
            None
        }
        Err(_) => {
            TRAY_STATE.store(2, Ordering::Relaxed);
            tracing::warn!("the tray icon could not be created (no AppIndicator library on this system)");
            None
        }
    }
}

fn show_window(desktop: &DesktopContext) {
    desktop.window.set_visible(true);
    desktop.window.set_minimized(false);
    desktop.window.set_focus();
}

fn quit(desktop: &DesktopContext) {
    // the last window closing ends the app (dioxus' `exit_on_last_window_close` default)
    desktop.set_close_behavior(WindowCloseBehaviour::WindowCloses);
    desktop.close();
}

/// Create the tray according to `Settings::system` and keep it in sync; start minimized; new-version notice.
/// Called once by `app::App`.
pub fn use_tray() {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let desktop = use_window();
    let state: Rc<RefCell<Option<TrayState>>> = use_hook(|| Rc::new(RefCell::new(None)));
    let start_checked: Rc<Cell<bool>> = use_hook(|| Rc::new(Cell::new(false)));

    // Menu: Show / Quit.
    {
        let state = state.clone();
        let desktop = desktop.clone();
        let _ = use_tray_menu_event_handler(move |e: &MenuEvent| {
            let (is_show, is_quit) = match state.borrow().as_ref() {
                Some(st) => (e.id == *st.show.id(), e.id == *st.quit.id()),
                None => (false, false),
            };
            if is_show {
                show_window(&desktop);
            } else if is_quit {
                quit(&desktop);
            }
        });
    }

    // A click on the icon restores the window (dioxus does it too on Windows / macOS; Linux has no icon clicks,
    // only the menu).
    {
        let desktop = desktop.clone();
        let _ = use_tray_icon_event_handler(move |e: &TrayIconEvent| match e {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => show_window(&desktop),
            _ => {}
        });
    }

    // Icon on / off, close behaviour, labels, start minimized.
    {
        let state = state.clone();
        let desktop = desktop.clone();
        use_effect(move || {
            let loaded = (ui.settings_loaded)();
            let prefs = ui.settings.read().system.clone();
            // subscribes to language changes (the menu labels follow them)
            let _ = t("tray.show");
            if !loaded {
                return;
            }
            let mut slot = state.borrow_mut();
            if prefs.tray_icon {
                if slot.is_none() {
                    *slot = build_tray();
                }
            } else {
                *slot = None;
            }
            if let Some(st) = slot.as_ref() {
                st.relabel();
            }
            let has_tray = slot.is_some();
            drop(slot);
            desktop.set_close_behavior(if has_tray && prefs.close_to_tray {
                WindowCloseBehaviour::WindowHides
            } else {
                WindowCloseBehaviour::WindowCloses
            });
            if !start_checked.get() {
                start_checked.set(true);
                if has_tray && prefs.start_minimized && has_minimized_arg(std::env::args_os()) {
                    desktop.window.set_visible(false);
                }
            }
        });
    }

    // New-version notice (once per launch).
    let mut release_url = use_signal(|| None::<String>);
    let open_release = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            let Some(url) = release_url.peek().clone() else { return };
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening the release page failed");
                }
            });
        }
    });
    let backend = services.backend.clone();
    use_future(move || {
        let backend = backend.clone();
        async move {
            while !*ui.settings_loaded.peek() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            tokio::time::sleep(Duration::from_secs(NOTICE_DELAY_SECS)).await;
            let prefs = ui.settings.peek().system.clone();
            if !prefs.check_updates || !prefs.setup_done {
                return;
            }
            match backend.check_updates().await {
                Ok(vm) if vm.newer => {
                    let Some(tag) = vm.latest.clone() else { return };
                    if !ui.settings.peek().system.should_notify(&tag) {
                        return;
                    }
                    release_url.set(vm.url.clone());
                    snack.show_action(tf("update.available", &[("version", &tag)]), t("update.open"), open_release);
                }
                Ok(_) => {}
                Err(e) => tracing::debug!(detail = %e.detail, "update check failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minimized_argument_is_only_read_after_the_program_name() {
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(has_minimized_arg(args(&["tatakai-next", "--minimized"])));
        assert!(!has_minimized_arg(args(&["--minimized"])));
        assert!(!has_minimized_arg(args(&["tatakai-next", "magnet:?xt=urn:btih:abc"])));
    }

    #[test]
    fn the_indicator_library_is_found_in_the_given_directories() {
        let tmp = std::env::temp_dir().join(format!("tk-tray-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(!indicator_lib_in(std::slice::from_ref(&tmp)));
        std::fs::write(tmp.join("libayatana-appindicator3.so.1"), b"").unwrap();
        assert!(indicator_lib_in(&[PathBuf::from("/nonexistent"), tmp.clone()]));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_icon_is_a_png() {
        assert!(ICON_PNG.starts_with(b"\x89PNG"));
    }
}
