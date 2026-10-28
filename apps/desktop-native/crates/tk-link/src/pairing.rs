//! Pairing window: 5 minutes, single use, at most 5 failed attempts, 10-character Crockford code, and the
//! `tatakai-link:v1?...` payload. Validation order: window -> CSR -> code (a bad CSR never consumes an attempt).
//! The code is never logged. Adapted from Enigmata-Temple `pairing.rs` (AGPL-3.0, same author).

use std::fmt;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use time::{Duration, OffsetDateTime};

use crate::api::ErrorCode;
use crate::devices::{Device, DeviceRegistry};
use crate::identity::Identity;

pub const PAYLOAD_PREFIX: &str = "tatakai-link:v1";
pub const CODE_LENGTH: usize = 10;
pub const DEFAULT_TTL: Duration = Duration::minutes(5);
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;
pub const MAX_DEVICE_NAME_CHARS: usize = 64;
pub const FALLBACK_DEVICE_NAME: &str = "unnamed device";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    Closed,
    AttemptsExceeded,
    CodeInvalid,
    CsrInvalid,
    Internal(&'static str),
}

impl PairingError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Closed => ErrorCode::PairingClosed,
            Self::AttemptsExceeded => ErrorCode::PairingAttemptsExceeded,
            Self::CodeInvalid => ErrorCode::PairingCodeInvalid,
            Self::CsrInvalid => ErrorCode::CsrInvalid,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code().message())
    }
}

/// What the UI shows (text and QR). Contains the pairing code: no `Debug` that prints it.
#[derive(Clone)]
pub struct PairingPayload {
    pub host: Ipv4Addr,
    pub port: u16,
    pub ca_fingerprint: String,
    pub code: String,
    pub expires_at: OffsetDateTime,
    pub server_name: String,
}

impl PairingPayload {
    /// `tatakai-link:v1?h=<ipv4>&p=<port>&ca=<sha256 hex>&c=<code>&x=<unix exp>&n=<name url-encoded>`
    pub fn uri(&self) -> String {
        let n = pct_encode(&self.server_name);
        format!(
            "{PAYLOAD_PREFIX}?h={}&p={}&ca={}&c={}&x={}&n={n}",
            self.host,
            self.port,
            self.ca_fingerprint,
            self.code,
            self.expires_at.unix_timestamp()
        )
    }

    pub fn qr_svg(&self) -> Result<String, PairingError> {
        let code = QrCode::with_error_correction_level(self.uri().as_bytes(), EcLevel::M).map_err(|_| PairingError::Internal("qr"))?;
        Ok(code.render::<svg::Color<'_>>().min_dimensions(256, 256).quiet_zone(true).build())
    }
}

/// RFC 3986 percent-encoding (everything except unreserved characters), so a space is `%20`, never `+`.
fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl fmt::Debug for PairingPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingPayload").field("host", &self.host).field("port", &self.port).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowStatus {
    Open { expires_at: OffsetDateTime, attempts_left: u32 },
    Closed,
    Exhausted,
}

enum WindowState {
    Closed,
    Open { code: [u8; CODE_LENGTH], expires_at: OffsetDateTime, failures: u32, max_attempts: u32 },
    Exhausted,
}

pub struct IssuedDevice {
    pub device_id: String,
    pub client_cert_pem: String,
    pub ca_cert_pem: String,
}

impl fmt::Debug for IssuedDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedDevice").field("device_id", &self.device_id).finish_non_exhaustive()
    }
}

pub struct PairingWindow {
    identity: Arc<Identity>,
    devices: Arc<DeviceRegistry>,
    host: Ipv4Addr,
    port: u16,
    server_name: String,
    state: Mutex<WindowState>,
}

impl PairingWindow {
    pub fn new(identity: Arc<Identity>, devices: Arc<DeviceRegistry>, host: Ipv4Addr, port: u16, server_name: String) -> Self {
        Self { identity, devices, host, port, server_name, state: Mutex::new(WindowState::Closed) }
    }

    fn lock(&self) -> MutexGuard<'_, WindowState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Open a fresh window (replacing any previous one) with a new code.
    pub fn open(&self, ttl: Duration, max_attempts: u32, now: OffsetDateTime) -> PairingPayload {
        let code = generate_code();
        let expires_at = OffsetDateTime::from_unix_timestamp(now.unix_timestamp().saturating_add(ttl.whole_seconds())).unwrap_or(now);
        *self.lock() = WindowState::Open { code, expires_at, failures: 0, max_attempts: max_attempts.max(1) };
        tracing::info!(expires_at = expires_at.unix_timestamp(), "pairing window opened");
        PairingPayload {
            host: self.host,
            port: self.port,
            ca_fingerprint: self.identity.ca_fingerprint(),
            code: String::from_utf8_lossy(&code).into_owned(),
            expires_at,
            server_name: self.server_name.clone(),
        }
    }

    pub fn close(&self) {
        let mut s = self.lock();
        if matches!(*s, WindowState::Open { .. }) {
            *s = WindowState::Closed;
        }
    }

    pub fn status(&self, now: OffsetDateTime) -> WindowStatus {
        match &*self.lock() {
            WindowState::Open { expires_at, failures, max_attempts, .. } if now < *expires_at => {
                WindowStatus::Open { expires_at: *expires_at, attempts_left: max_attempts - failures }
            }
            WindowState::Exhausted => WindowStatus::Exhausted,
            _ => WindowStatus::Closed,
        }
    }

    /// Redeem `code` at `now`. The window closes BEFORE the certificate is issued, so there is never a second redeem.
    pub fn redeem(&self, code: &str, csr_pem: &str, device_name: &str, now: OffsetDateTime) -> Result<IssuedDevice, PairingError> {
        let mut state = self.lock();
        // 1. window
        match &*state {
            WindowState::Open { expires_at, .. } if now < *expires_at => {}
            WindowState::Open { .. } => {
                *state = WindowState::Closed;
                return Err(PairingError::Closed);
            }
            WindowState::Exhausted => return Err(PairingError::AttemptsExceeded),
            WindowState::Closed => return Err(PairingError::Closed),
        }
        // 2. CSR: validated (signature, P-256) without consuming an attempt; the certificate is issued only after the code
        //    matched, from this same CSR.
        if !csr_is_acceptable(csr_pem) {
            return Err(PairingError::CsrInvalid);
        }
        // 3. code (constant time)
        let WindowState::Open { code: expected, failures, max_attempts, .. } = &mut *state else { return Err(PairingError::Closed) };
        let given = normalize_code(code);
        let ok = given.is_some_and(|g| ct_eq(expected, &g));
        if !ok {
            *failures += 1;
            if *failures >= *max_attempts {
                *state = WindowState::Exhausted;
                tracing::warn!("pairing window closed after too many failed attempts");
                return Err(PairingError::AttemptsExceeded);
            }
            return Err(PairingError::CodeInvalid);
        }
        *state = WindowState::Closed; // single use
        drop(state);

        let cert = self.identity.issue_client_from_csr(csr_pem, now).map_err(|e| {
            tracing::error!(error = %e, "client certificate could not be issued");
            PairingError::Internal("certificate issuance failed")
        })?;
        self.devices
            .add(Device {
                id: cert.device_id.clone(),
                name: sanitize_device_name(device_name),
                fingerprint: cert.fingerprint.clone(),
                created_at: now.unix_timestamp(),
                not_after: cert.not_after.unix_timestamp(),
                revoked_at: None,
            })
            .map_err(|_| PairingError::Internal("device registration failed"))?;
        tracing::info!(device = %cert.device_id, "device paired");
        Ok(IssuedDevice { device_id: cert.device_id, client_cert_pem: cert.pem, ca_cert_pem: self.identity.ca_pem().to_owned() })
    }
}

fn csr_is_acceptable(csr_pem: &str) -> bool {
    use rcgen::{CertificateSigningRequestParams, PKCS_ECDSA_P256_SHA256};
    use rustls::pki_types::CertificateSigningRequestDer;
    use rustls::pki_types::pem::PemObject;
    let Ok(der) = CertificateSigningRequestDer::from_pem_slice(csr_pem.as_bytes()) else { return false };
    CertificateSigningRequestParams::from_der(&der).is_ok_and(|c| c.public_key.algorithm() == &PKCS_ECDSA_P256_SHA256)
}

fn ct_eq(a: &[u8; CODE_LENGTH], b: &[u8; CODE_LENGTH]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn generate_code() -> [u8; CODE_LENGTH] {
    let bytes = uuid::Uuid::new_v4();
    let mut bits = u64::from_le_bytes(bytes.as_bytes()[..8].try_into().unwrap_or([0; 8]));
    let mut code = [0u8; CODE_LENGTH];
    for slot in &mut code {
        *slot = CROCKFORD[(bits & 0x1f) as usize];
        bits >>= 5;
    }
    code
}

/// Crockford reading: case-insensitive, `O`->0, `I`/`L`->1, dashes ignored. `None` unless exactly 10 valid symbols.
pub fn normalize_code(input: &str) -> Option<[u8; CODE_LENGTH]> {
    let mut code = [0u8; CODE_LENGTH];
    let mut len = 0;
    for c in input.trim().chars() {
        let symbol = match c.to_ascii_uppercase() {
            '-' => continue,
            'O' => b'0',
            'I' | 'L' => b'1',
            other => u8::try_from(other).ok()?,
        };
        if !CROCKFORD.contains(&symbol) || len == CODE_LENGTH {
            return None;
        }
        code[len] = symbol;
        len += 1;
    }
    (len == CODE_LENGTH).then_some(code)
}

/// Display-safe device name: no control/format characters, collapsed whitespace, at most 64 characters.
pub fn sanitize_device_name(name: &str) -> String {
    let mut clean = String::new();
    let mut pending_space = false;
    for c in name.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if c.is_control() || is_format_char(c) {
            continue;
        }
        if pending_space && !clean.is_empty() {
            clean.push(' ');
        }
        pending_space = false;
        clean.push(c);
    }
    let t: String = clean.chars().take(MAX_DEVICE_NAME_CHARS).collect();
    let t = t.trim_end();
    if t.is_empty() { FALLBACK_DEVICE_NAME.to_owned() } else { t.to_owned() }
}

fn is_format_char(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};

    fn setup() -> (tempfile::TempDir, PairingWindow, Arc<DeviceRegistry>) {
        let dir = tempfile::tempdir().unwrap();
        let identity = Arc::new(Identity::load_or_init(dir.path(), &[], OffsetDateTime::now_utc()).unwrap());
        let devices = Arc::new(DeviceRegistry::in_memory());
        let w = PairingWindow::new(identity, devices.clone(), Ipv4Addr::new(192, 168, 1, 20), 47321, "My PC & Co".into());
        (dir, w, devices)
    }

    fn csr() -> String {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        CertificateParams::default().serialize_request(&key).unwrap().pem().unwrap()
    }

    fn t(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000 + secs).unwrap()
    }

    #[test]
    fn payload_has_the_exact_format() {
        let (_d, w, _) = setup();
        let p = w.open(DEFAULT_TTL, 5, t(0));
        assert_eq!(p.code.len(), 10);
        assert!(p.code.bytes().all(|b| CROCKFORD.contains(&b)));
        let uri = p.uri();
        assert!(
            uri.starts_with(&format!(
                "tatakai-link:v1?h=192.168.1.20&p=47321&ca={}&c={}&x={}&n=",
                p.ca_fingerprint,
                p.code,
                1_800_000_300
            )),
            "{uri}"
        );
        assert!(uri.ends_with("&n=My%20PC%20%26%20Co"), "{uri}");
        assert!(p.qr_svg().unwrap().contains("<svg"));
        assert!(!format!("{p:?}").contains(&p.code), "Debug never prints the code");
    }

    #[test]
    fn code_is_read_as_crockford() {
        let c = *b"0123456789";
        assert_eq!(normalize_code("0123456789"), Some(c));
        assert_eq!(normalize_code("o1i3-4567 89"), None, "inner spaces are not accepted");
        assert_eq!(normalize_code("OIL3456789"), Some(*b"0113456789"));
        assert_eq!(normalize_code("01234-56789"), Some(c));
        assert_eq!(normalize_code("012345678"), None);
        assert_eq!(normalize_code("01234567890"), None);
        assert_eq!(normalize_code("0123456U89"), None, "U is not in the alphabet");
    }

    #[test]
    fn redeems_once_and_registers_the_device() {
        let (_d, w, devices) = setup();
        let p = w.open(DEFAULT_TTL, 5, t(0));
        let issued = w.redeem(&p.code, &csr(), "  My\u{202e} Phone  ", t(10)).unwrap();
        assert!(issued.client_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(issued.ca_cert_pem.contains("BEGIN CERTIFICATE"));
        let d = devices.get(&issued.device_id).unwrap();
        assert_eq!(d.name, "My Phone");
        assert!(devices.is_active(&issued.device_id, t(11).unix_timestamp()));
        // single use: the window is closed even with the right code
        assert_eq!(w.redeem(&p.code, &csr(), "x", t(11)).unwrap_err(), PairingError::Closed);
        assert_eq!(w.status(t(11)), WindowStatus::Closed);
    }

    #[test]
    fn five_wrong_codes_close_the_window_even_for_the_right_one() {
        let (_d, w, devices) = setup();
        let p = w.open(DEFAULT_TTL, 5, t(0));
        for left in (1..5).rev() {
            assert_eq!(w.redeem("0000000000", &csr(), "x", t(1)).unwrap_err(), PairingError::CodeInvalid);
            assert_eq!(w.status(t(1)), WindowStatus::Open { expires_at: t(300), attempts_left: left });
        }
        assert_eq!(w.redeem("0000000000", &csr(), "x", t(1)).unwrap_err(), PairingError::AttemptsExceeded);
        assert_eq!(w.redeem(&p.code, &csr(), "x", t(2)).unwrap_err(), PairingError::AttemptsExceeded);
        assert_eq!(w.status(t(2)), WindowStatus::Exhausted);
        assert!(devices.list().is_empty());
    }

    #[test]
    fn expiry_and_validation_order() {
        let (_d, w, devices) = setup();
        // closed window -> Closed, regardless of the CSR/code
        assert_eq!(w.redeem("x", "garbage", "x", t(0)).unwrap_err(), PairingError::Closed);
        let p = w.open(DEFAULT_TTL, 5, t(0));
        // window -> CSR -> code: a bad CSR with a wrong code reports the CSR and costs no attempt
        assert_eq!(w.redeem("wrong", "garbage", "x", t(1)).unwrap_err(), PairingError::CsrInvalid);
        assert_eq!(w.status(t(1)), WindowStatus::Open { expires_at: t(300), attempts_left: 5 });
        // a bad CSR with the right code does not redeem either
        assert_eq!(w.redeem(&p.code, "garbage", "x", t(1)).unwrap_err(), PairingError::CsrInvalid);
        // exactly at expiry the window is closed
        assert_eq!(w.redeem(&p.code, &csr(), "x", t(300)).unwrap_err(), PairingError::Closed);
        assert_eq!(w.status(t(299)), WindowStatus::Closed);
        assert!(devices.list().is_empty());
        // a fresh window works again and has a different code
        let p2 = w.open(DEFAULT_TTL, 5, t(400));
        assert_ne!(p.code, p2.code);
        w.redeem(&p2.code, &csr(), "x", t(401)).unwrap();
    }

    #[test]
    fn rejects_rsa_or_foreign_algorithm_csrs() {
        let (_d, w, _) = setup();
        let p = w.open(DEFAULT_TTL, 5, t(0));
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P384_SHA384).unwrap();
        let csr384 = CertificateParams::default().serialize_request(&key).unwrap().pem().unwrap();
        assert_eq!(w.redeem(&p.code, &csr384, "x", t(1)).unwrap_err(), PairingError::CsrInvalid);
    }

    #[test]
    fn device_names_are_cleaned() {
        assert_eq!(sanitize_device_name("  Pixel \u{200b}8\n"), "Pixel 8");
        assert_eq!(sanitize_device_name("\u{202e}\u{0007}"), FALLBACK_DEVICE_NAME);
        assert_eq!(sanitize_device_name(&"a".repeat(100)).chars().count(), 64);
    }
}
