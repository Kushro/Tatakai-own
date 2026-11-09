//! Start with the system (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 4; classic
//! `desktop/ipc/ipc-system.cjs` `set-auto-launch` / `get-auto-launch`) through the `auto-launch` crate:
//! `AutoLaunchBuilder::new().set_app_name("Tatakai Next").set_app_path(<current exe>).set_args(&["--minimized"])`,
//! then `enable` / `disable` / `is_enabled`. Linux writes `~/.config/autostart/Tatakai Next.desktop` (XDG
//! autostart), Windows the HKCU `Run` value, macOS a LaunchAgent. `--minimized` is honoured by `tk_ui::tray` when
//! `Settings::system.start_minimized` (window hidden, tray only). No elevation, per user.

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// Name of the autostart entry (file name on Linux, value name on Windows, label on macOS).
pub const APP_NAME: &str = "Tatakai Next";
/// Argument the autostart entry passes (the app starts hidden in the tray when the user asked for it).
pub const MINIMIZED_ARG: &str = "--minimized";

/// Whether this platform can register an autostart entry (Linux, Windows, macOS).
pub fn supported() -> bool {
    AutoLaunch::is_support() && current_exe().is_some()
}

/// Whether the OS entry exists right now (it may differ from `Settings::system.start_with_system` when the user
/// removed it by hand).
pub fn is_enabled() -> Result<bool, String> {
    launcher()?.is_enabled().map_err(|e| e.to_string())
}

/// Create (`true`) or remove (`false`) the OS entry. Idempotent.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let l = launcher()?;
    let now = l.is_enabled().unwrap_or(!enabled);
    if now == enabled {
        return Ok(());
    }
    let done = if enabled { l.enable() } else { l.disable() };
    done.map_err(|e| e.to_string())
}

fn current_exe() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    // `current_exe` may be a symlink (AppImage mounts, `/usr/bin` links): register the real file.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    exe.to_str().map(str::to_owned)
}

fn launcher() -> Result<AutoLaunch, String> {
    if !AutoLaunch::is_support() {
        return Err("autostart is not supported on this platform".to_owned());
    }
    let exe = current_exe().ok_or_else(|| "cannot determine the path of this executable".to_owned())?;
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe)
        .set_args(&[MINIMIZED_ARG])
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_builds_for_this_executable() {
        // Building only describes the entry; nothing is written until `enable`.
        if AutoLaunch::is_support() {
            let l = launcher().unwrap();
            assert_eq!(l.get_app_name(), APP_NAME);
            assert_eq!(l.get_args(), [MINIMIZED_ARG.to_owned()]);
            assert!(supported());
        }
    }
}
