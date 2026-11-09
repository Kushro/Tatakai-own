//! mDNS announcement of Tatakai Link (wave B, worker **k**; checklist `link.discovery`; docs/redesign/03 listed it as
//! out of M0). Service type [`SERVICE_TYPE`], instance = the server name, port = the Link port (SRV record), TXT
//! records `v=1`, `name=<server name>`, `id=<instance_id of /v1/capabilities>`, `fp=<sha256 of the Link CA certificate,
//! hex>`. CANONICAL for the Android client (worker r, `NsdManager`): same service type and TXT keys, host and port from
//! the resolved service so the phone can list PCs and confirm the fingerprint on first use
//! (TOFU) before pairing. Crate `mdns-sd` (pure Rust, no daemon; docs/redesign/16 section 3.7).
//! [`Announcer::start`] registers (`Drop` unregisters). `fp` is the fingerprint of the Link CA
//! (`LinkHandle::ca_fingerprint`), the certificate the phone pins after pairing. The app (`tk-app/src/live/link.rs`)
//! starts it next to the server while Settings > Link "Announce on the network" is on (default on), so
//! `tk_link::start` / `LinkConfig` need no change.

/// `_tatakai-link._tcp.local.`
pub const SERVICE_TYPE: &str = "_tatakai-link._tcp.local.";

/// TXT record keys.
pub const TXT_VERSION: &str = "v";
pub const TXT_NAME: &str = "name";
pub const TXT_INSTANCE_ID: &str = "id";
pub const TXT_FINGERPRINT: &str = "fp";

/// The TXT records of the announcement, in the canonical key order (`v`, `name`, `id`, `fp`). Pure.
pub fn txt_records(server_name: &str, instance_id: &str, fingerprint_hex: &str) -> Vec<(String, String)> {
    let name: String = server_name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    vec![
        (TXT_VERSION.to_owned(), "1".to_owned()),
        (TXT_NAME.to_owned(), if name.is_empty() { "Tatakai".to_owned() } else { name }),
        (TXT_INSTANCE_ID.to_owned(), instance_id.to_owned()),
        (TXT_FINGERPRINT.to_owned(), fingerprint_hex.to_ascii_lowercase()),
    ]
}

/// mDNS host name of this PC for the SRV record: `tatakai-<first 12 alphanumerics of the instance id>.local.`
/// (unique per install, no personal data). Pure.
pub fn host_label(instance_id: &str) -> String {
    let id: String = instance_id.chars().filter(char::is_ascii_alphanumeric).take(12).collect::<String>().to_ascii_lowercase();
    format!("tatakai-{}.local.", if id.is_empty() { "pc".to_owned() } else { id })
}

/// A registered `_tatakai-link._tcp` service. Dropping it unregisters the service and stops the responder.
pub struct Announcer {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl Announcer {
    /// Announce on every IPv4 interface (addresses picked by the responder).
    pub fn start(server_name: &str, instance_id: &str, port: u16, fingerprint_hex: &str) -> Result<Self, String> {
        Self::start_on(server_name, instance_id, port, fingerprint_hex, &[])
    }

    /// Announce with the given addresses (the ones the Link server listens on); empty = automatic.
    pub fn start_on(server_name: &str, instance_id: &str, port: u16, fingerprint_hex: &str, ips: &[std::net::IpAddr]) -> Result<Self, String> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
        // Link listens on IPv4 only (`LinkConfig::bind_ips`).
        let _ = daemon.disable_interface(mdns_sd::IfKind::IPv6);
        let txt = txt_records(server_name, instance_id, fingerprint_hex);
        let instance = instance_name(server_name);
        let host = host_label(instance_id);
        let usable: Vec<std::net::IpAddr> = ips.iter().copied().filter(|ip| ip.is_ipv4() && !ip.is_loopback() && !ip.is_unspecified()).collect();
        let info = if usable.is_empty() {
            mdns_sd::ServiceInfo::new(SERVICE_TYPE, &instance, &host, (), port, txt.as_slice()).map(mdns_sd::ServiceInfo::enable_addr_auto)
        } else {
            mdns_sd::ServiceInfo::new(SERVICE_TYPE, &instance, &host, usable.as_slice(), port, txt.as_slice())
        }
        .map_err(|e| e.to_string())?;
        let fullname = info.get_fullname().to_owned();
        daemon.register(info).map_err(|e| e.to_string())?;
        tracing::info!(service = %fullname, port, "Tatakai Link announced over mDNS");
        Ok(Self { daemon, fullname })
    }

    /// The full service name (`<instance>._tatakai-link._tcp.local.`).
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

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
    fn txt_records_use_the_canonical_keys() {
        let txt = super::txt_records(" My PC ", "abc-123", "ABCDEF");
        assert_eq!(
            txt,
            vec![("v".into(), "1".into()), ("name".into(), "My PC".into()), ("id".into(), "abc-123".into()), ("fp".into(), "abcdef".into())]
        );
        assert_eq!(super::host_label("3f2a-91c0-77aa-bb11-0000"), "tatakai-3f2a91c077aa.local.");
        assert_eq!(super::host_label("--"), "tatakai-pc.local.");
    }

    #[test]
    fn instance_names_are_bounded_and_dotless() {
        assert_eq!(super::instance_name("My.PC"), "MyPC");
        assert_eq!(super::instance_name("   "), "Tatakai");
        assert!(super::instance_name(&"x".repeat(100)).len() <= 63);
    }
}
