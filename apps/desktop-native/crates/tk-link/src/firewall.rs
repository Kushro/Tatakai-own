//! Windows Firewall rule (Private profile only). Creating it needs elevation, so it happens ONLY when the UI calls
//! [`request_private_network_rule`] from Settings > Link > "Allow on private network" (UAC prompt) - never on first
//! launch, never automatically, never for the Public profile (review E6).

use std::io;
use std::path::Path;

pub const RULE_NAME: &str = "Tatakai Link";

/// `netsh` arguments of the inbound rule.
pub fn add_rule_args(exe: &Path, port: u16) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={RULE_NAME}"),
        "dir=in".into(),
        "action=allow".into(),
        "protocol=TCP".into(),
        format!("localport={port}"),
        "profile=private".into(),
        format!("program={}", exe.display()),
    ]
}

pub fn delete_rule_args() -> Vec<String> {
    vec!["advfirewall".into(), "firewall".into(), "delete".into(), "rule".into(), format!("name={RULE_NAME}")]
}

/// The PowerShell command line that elevates `netsh` with `args` (UAC prompt) and waits for it.
pub fn elevated_powershell_command(args: &[String]) -> String {
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{}\"", a.replace('"', "'"))).collect();
    format!(
        "Start-Process -FilePath netsh -ArgumentList '{}' -Verb RunAs -Wait -WindowStyle Hidden",
        quoted.join(" ").replace('\'', "''")
    )
}

fn run_elevated(args: &[String]) -> io::Result<()> {
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &elevated_powershell_command(args)])
        .status()?;
    if status.success() { Ok(()) } else { Err(io::Error::new(io::ErrorKind::PermissionDenied, "the elevated netsh command did not succeed (cancelled?)")) }
}

/// Add the Private-profile inbound rule for `exe` on `port` (shows a UAC prompt). Blocking: run in `spawn_blocking`.
pub fn request_private_network_rule(exe: &Path, port: u16) -> io::Result<()> {
    run_elevated(&add_rule_args(exe, port))
}

/// Remove the rule (UAC prompt).
pub fn remove_rule() -> io::Result<()> {
    run_elevated(&delete_rule_args())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_is_private_profile_inbound_tcp_on_the_link_port_only() {
        let a = add_rule_args(Path::new("C:\\Apps\\Tatakai Next\\tatakai-next.exe"), 47321);
        assert!(a.contains(&"profile=private".to_owned()));
        assert!(a.contains(&"dir=in".to_owned()));
        assert!(a.contains(&"protocol=TCP".to_owned()));
        assert!(a.contains(&"localport=47321".to_owned()));
        assert!(!a.iter().any(|x| x.contains("public") || x.contains("any") || x.contains("profile=all")));
        assert!(a.iter().any(|x| x.starts_with("program=") && x.ends_with("tatakai-next.exe")));
        let ps = elevated_powershell_command(&a);
        assert!(ps.contains("-Verb RunAs") && ps.starts_with("Start-Process -FilePath netsh"));
        assert!(elevated_powershell_command(&delete_rule_args()).contains("delete"));
    }
}
