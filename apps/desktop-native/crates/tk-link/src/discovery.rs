//! mDNS announcement of Tatakai Link (wave B, worker **k**; checklist `link.discovery`; docs/redesign/03 listed it as
//! out of M0). Service type [`SERVICE_TYPE`], instance = the server name, port = the Link port (SRV record), TXT
//! records `v=1`, `name=<server name>`, `id=<instance_id of /v1/capabilities>`, `fp=<sha256 of the server certificate,
//! hex>`. CANONICAL for the Android client (worker r, `NsdManager`): same service type and TXT keys, host and port from
//! the resolved service so the phone can list PCs and confirm the fingerprint on first use
//! (TOFU) before pairing. Crate `mdns-sd` (pure Rust, no daemon; docs/redesign/16 section 3.7). k implements
//! `Announcer::start(name, instance_id, port, fingerprint_hex) -> Announcer` (registers; `Drop` unregisters) and wires it from
//! `tk_link::start` behind `LinkConfig::announce` (default `true`; Settings > Link gets the switch).

/// `_tatakai-link._tcp.local.`
pub const SERVICE_TYPE: &str = "_tatakai-link._tcp.local.";

/// TXT record keys.
pub const TXT_VERSION: &str = "v";
pub const TXT_NAME: &str = "name";
pub const TXT_INSTANCE_ID: &str = "id";
pub const TXT_FINGERPRINT: &str = "fp";

/// Instance name as mDNS allows it (<= 63 bytes, no dots); pure.
pub fn instance_name(server_name: &str) -> String {
    let cleaned: String = server_name.trim().chars().filter(|c| *c != '.').collect();
    let cleaned = if cleaned.is_empty() { "Tatakai".to_owned() } else { cleaned };
    let mut out = String::new();
    for ch in cleaned.chars() {
        if out.len() + ch.len_utf8() > 63 {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn instance_names_are_bounded_and_dotless() {
        assert_eq!(super::instance_name("My.PC"), "MyPC");
        assert_eq!(super::instance_name("   "), "Tatakai");
        assert!(super::instance_name(&"x".repeat(100)).len() <= 63);
    }
}
