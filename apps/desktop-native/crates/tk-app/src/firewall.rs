//! "Allow on private network" (review E6): the Windows Firewall rule needs elevation, so it is created
//! ONLY when the user presses the button in Settings > Link, through a UAC prompt - never at first
//! launch and never for the Public profile.

use std::path::Path;

pub const RULE_NAME: &str = "Tatakai Link";
pub const DEFAULT_PORT: u16 = 47321;

/// `netsh advfirewall firewall add rule ...` arguments (private profile only, inbound TCP, this exe).
pub fn netsh_args(exe: &Path, port: u16) -> Vec<String> {
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
        "enable=yes".into(),
    ]
}

/// Quote a value as a PowerShell single-quoted literal.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// One netsh argument as it must appear on the command line: values with spaces are wrapped in double quotes
/// (`program="C:\Program Files\app.exe"`), because `Start-Process -ArgumentList` does not quote for us.
pub fn cmdline_arg(a: &str) -> String {
    match a.split_once('=') {
        Some((k, v)) if a.contains(' ') => format!("{k}=\"{v}\""),
        _ => a.to_owned(),
    }
}

/// The PowerShell script that elevates `netsh` (UAC) and waits for it. Exit code 0 only if netsh succeeded.
pub fn elevate_script(exe: &Path, port: u16) -> String {
    let args = netsh_args(exe, port).iter().map(|a| ps_quote(&cmdline_arg(a))).collect::<Vec<_>>().join(",");
    format!(
        "$ErrorActionPreference='Stop'; try {{ $p = Start-Process -FilePath 'netsh.exe' -ArgumentList @({args}) -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}"
    )
}

/// Runs the elevation flow. Blocking: call from `spawn_blocking`.
pub fn allow_private_network(exe: &Path, port: u16) -> Result<(), String> {
    let script = elevate_script(exe, port);
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let status = cmd.status().map_err(|e| format!("could not start PowerShell: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err("the administrator prompt was cancelled".into()),
        other => Err(format!("netsh failed (exit code {other:?})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_is_private_inbound_tcp_for_the_exe_only() {
        let args = netsh_args(Path::new("C:/Apps/Tatakai/tatakai-next.exe"), 47321);
        assert!(args.contains(&"profile=private".to_string()));
        assert!(args.contains(&"dir=in".to_string()));
        assert!(args.contains(&"protocol=TCP".to_string()));
        assert!(args.contains(&"localport=47321".to_string()));
        assert!(args.iter().any(|a| a.starts_with("program=") && a.ends_with("tatakai-next.exe")));
        assert!(!args.iter().any(|a| a.contains("public") || a.contains("any")));
    }

    #[test]
    fn script_elevates_netsh_and_escapes_quotes() {
        let s = elevate_script(Path::new("C:/Users/O'Neil/app/tatakai-next.exe"), 47321);
        assert!(s.contains("-Verb RunAs"));
        assert!(s.contains("netsh.exe"));
        assert!(s.contains("O''Neil"));
        assert!(s.contains("exit 1223"));
    }

    #[test]
    fn paths_with_spaces_are_quoted_for_netsh() {
        assert_eq!(cmdline_arg("program=C:/Program Files/T/t.exe"), "program=\"C:/Program Files/T/t.exe\"");
        assert_eq!(cmdline_arg("profile=private"), "profile=private");
        let s = elevate_script(Path::new("C:/Program Files/T/t.exe"), 47321);
        assert!(s.contains("'program=\"C:/Program Files/T/t.exe\"'"), "{s}");
    }

    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote("a'b"), "'a''b'");
    }
}
