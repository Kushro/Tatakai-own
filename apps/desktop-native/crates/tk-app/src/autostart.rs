//! Start with the system (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 4) through the
//! `auto-launch` crate: `AutoLaunchBuilder::new().set_app_name("Tatakai Next").set_app_path(<current exe>)
//! .set_args(&["--minimized"]).build()`, then `enable` / `disable` / `is_enabled`. Linux writes
//! `~/.config/autostart/Tatakai Next.desktop`, Windows the HKCU `Run` value, macOS a LaunchAgent. `--minimized`
//! is honoured by `main.rs` when `Settings::system.start_minimized` (window hidden, tray only). Contract stubs.

/// Whether this platform / build can register an autostart entry.
pub fn supported() -> bool {
    false
}

/// Contract stub (worker f).
pub fn is_enabled() -> Result<bool, String> {
    Ok(false)
}

/// Contract stub (worker f).
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let _ = enabled;
    Err("autostart is not wired in this build".to_owned())
}
