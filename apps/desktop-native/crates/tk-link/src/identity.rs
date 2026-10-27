//! Own CA + server certificate (rcgen on the `ring` provider) and client-certificate issuance from a CSR.
//!
//! Adapted from Enigmata-Temple `temple-core/src/identity.rs` (AGPL-3.0, same author); aws-lc-rs replaced by ring/uuid
//! randomness and the key files protected with DPAPI. Files in the injected link dir: `ca.der`, `ca.key` (DPAPI),
//! `server.crt.pem`, `server.key` (DPAPI).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType, SerialNumber,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, CertificateSigningRequestDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use x509_parser::certificate::X509Certificate;
use x509_parser::extensions::GeneralName;

use crate::protect;

pub const CA_VALIDITY: Duration = Duration::days(3650);
pub const SERVER_VALIDITY: Duration = Duration::days(365);
pub const CLIENT_VALIDITY: Duration = Duration::days(365);
pub const SERVER_RENEWAL_MARGIN: Duration = Duration::days(30);
/// Back-dating tolerates clock skew between PC and phone.
const BACKDATE: Duration = Duration::hours(1);

const CA_CERT_FILE: &str = "ca.der";
const CA_KEY_FILE: &str = "ca.key";
const SERVER_CERT_FILE: &str = "server.crt.pem";
const SERVER_KEY_FILE: &str = "server.key";
const ORG: &str = "Tatakai Link";

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid identity material: {0}")]
    Invalid(&'static str),
    #[error("certificate generation failed: {0}")]
    Certificate(#[from] rcgen::Error),
    #[error("identity I/O error: {0}")]
    Io(#[from] io::Error),
}

/// SHA-256 of a DER blob (the CA pin and the device fingerprints).
pub fn sha256_hex(der: &[u8]) -> String {
    hex::encode(Sha256::digest(der))
}

pub struct ClientCertificate {
    pub device_id: String,
    pub der: CertificateDer<'static>,
    pub pem: String,
    pub fingerprint: String,
    pub not_after: OffsetDateTime,
}

pub struct Identity {
    ca_der: CertificateDer<'static>,
    ca_pem: String,
    issuer: Issuer<'static, KeyPair>,
    server_der: CertificateDer<'static>,
    server_key: KeyPair,
}

impl Identity {
    /// Load the identity from `dir` or create it (and re-issue the server certificate when the IP set changed or it is
    /// about to expire). `dir` is injected: tests pass a tempdir.
    pub fn load_or_init(dir: &Path, server_ips: &[IpAddr], now: OffsetDateTime) -> Result<Self, IdentityError> {
        fs::create_dir_all(dir)?;
        let ca_cert_path = dir.join(CA_CERT_FILE);
        let ca_key_path = dir.join(CA_KEY_FILE);
        if !ca_cert_path.exists() || !ca_key_path.exists() {
            return Self::init(dir, server_ips, now);
        }
        let ca_der = CertificateDer::from(fs::read(&ca_cert_path)?);
        let ca_key = KeyPair::try_from(protect::unprotect(&fs::read(&ca_key_path)?)?.as_slice())
            .map_err(|_| IdentityError::Invalid("CA key is unreadable"))?;
        if !key_matches(&ca_der, &ca_key) {
            return Err(IdentityError::Invalid("CA key does not match its certificate"));
        }
        let ca_pem = pem_of("CERTIFICATE", &ca_der);
        let issuer = Issuer::from_ca_cert_der(&ca_der, ca_key)?;
        let wanted = ip_set(server_ips);
        let existing = (|| -> Option<(CertificateDer<'static>, KeyPair)> {
            let cert = CertificateDer::from_pem_slice(fs::read(dir.join(SERVER_CERT_FILE)).ok()?.as_slice()).ok()?;
            let key = KeyPair::try_from(protect::unprotect(&fs::read(dir.join(SERVER_KEY_FILE)).ok()?).ok()?.as_slice()).ok()?;
            Some((cert, key))
        })();
        let mut me = match existing {
            Some((server_der, server_key)) => Self { ca_der, ca_pem, issuer, server_der, server_key },
            None => {
                let (c, k) = issue_server(&issuer, &wanted, now)?;
                let me = Self { ca_der, ca_pem, issuer, server_der: c.der().clone(), server_key: k };
                me.persist_server(dir, &c)?;
                me
            }
        };
        me.ensure_server_certificate(dir, server_ips, now)?;
        Ok(me)
    }

    fn init(dir: &Path, server_ips: &[IpAddr], now: OffsetDateTime) -> Result<Self, IdentityError> {
        let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let ca_cert = ca_params(now).self_signed(&ca_key)?;
        fs::write(dir.join(CA_CERT_FILE), ca_cert.der().as_ref())?;
        fs::write(dir.join(CA_KEY_FILE), protect::protect(&ca_key.serialize_der())?)?;
        let ca_pem = ca_cert.pem();
        let issuer = Issuer::from_ca_cert_der(ca_cert.der(), ca_key)?;
        let (server_cert, server_key) = issue_server(&issuer, &ip_set(server_ips), now)?;
        let me = Self { ca_der: ca_cert.der().clone(), ca_pem, issuer, server_der: server_cert.der().clone(), server_key };
        me.persist_server(dir, &server_cert)?;
        Ok(me)
    }

    fn persist_server(&self, dir: &Path, cert: &Certificate) -> Result<(), IdentityError> {
        fs::write(dir.join(SERVER_CERT_FILE), cert.pem())?;
        fs::write(dir.join(SERVER_KEY_FILE), protect::protect(&self.server_key.serialize_der())?)?;
        Ok(())
    }

    /// Re-issue the server certificate (the CA never changes) if its SANs differ from `server_ips` + 127.0.0.1, it
    /// expires within the renewal margin or it is not signed by our CA. Returns whether it was re-issued.
    pub fn ensure_server_certificate(&mut self, dir: &Path, server_ips: &[IpAddr], now: OffsetDateTime) -> Result<bool, IdentityError> {
        let wanted = ip_set(server_ips);
        if !self.server_needs_renewal(&wanted, now) {
            return Ok(false);
        }
        let (cert, key) = issue_server(&self.issuer, &wanted, now)?;
        self.server_der = cert.der().clone();
        self.server_key = key;
        self.persist_server(dir, &cert)?;
        Ok(true)
    }

    pub fn ca_fingerprint(&self) -> String {
        sha256_hex(&self.ca_der)
    }

    pub fn ca_der(&self) -> &CertificateDer<'static> {
        &self.ca_der
    }

    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    /// Leaf + CA, so the client can check the CA pin during pairing.
    pub fn server_chain(&self) -> Vec<CertificateDer<'static>> {
        vec![self.server_der.clone(), self.ca_der.clone()]
    }

    pub fn server_key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.server_key.serialize_der()))
    }

    pub fn server_ips(&self) -> BTreeSet<IpAddr> {
        parse(&self.server_der).map(|c| san_ips(&c)).unwrap_or_default()
    }

    /// Issue a client certificate for the public key of a (signature-verified) CSR. Only ECDSA P-256 keys are accepted.
    /// The requested subject/extensions are ignored; the server assigns `device_id` as CN.
    pub fn issue_client_from_csr(&self, csr_pem: &str, now: OffsetDateTime) -> Result<ClientCertificate, IdentityError> {
        let der = CertificateSigningRequestDer::from_pem_slice(csr_pem.as_bytes()).map_err(|_| IdentityError::Invalid("CSR is not PEM"))?;
        // `from_der` verifies the CSR self-signature.
        let csr = CertificateSigningRequestParams::from_der(&der).map_err(|_| IdentityError::Invalid("CSR is malformed or its signature is wrong"))?;
        if rcgen::PublicKeyData::algorithm(&csr.public_key) != &PKCS_ECDSA_P256_SHA256 {
            return Err(IdentityError::Invalid("client key must be ECDSA P-256"));
        }
        let device_id = uuid::Uuid::new_v4().to_string();
        let not_before = whole_seconds(now - BACKDATE);
        let not_after = whole_seconds(now + CLIENT_VALIDITY);
        let mut params = CertificateParams::default();
        params.distinguished_name = dn(&device_id);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.use_authority_key_identifier_extension = true;
        params.serial_number = Some(SerialNumber::from_slice(&random_serial()));
        params.not_before = not_before;
        params.not_after = not_after;
        let cert = params.signed_by(&csr.public_key, &self.issuer)?;
        Ok(ClientCertificate { device_id, fingerprint: sha256_hex(cert.der()), der: cert.der().clone(), pem: cert.pem(), not_after })
    }

    fn server_needs_renewal(&self, wanted: &BTreeSet<IpAddr>, now: OffsetDateTime) -> bool {
        let (Some(server), Some(ca)) = (parse(&self.server_der), parse(&self.ca_der)) else { return true };
        let renew_at = server.validity().not_after.timestamp() - SERVER_RENEWAL_MARGIN.whole_seconds();
        now.unix_timestamp() >= renew_at
            || san_ips(&server) != *wanted
            || !key_matches(&self.server_der, &self.server_key)
            || server.verify_signature(Some(ca.public_key())).is_err()
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").field("ca", &self.ca_fingerprint()).finish_non_exhaustive()
    }
}

fn pem_of(label: &str, der: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

fn ip_set(ips: &[IpAddr]) -> BTreeSet<IpAddr> {
    let mut s: BTreeSet<IpAddr> = ips.iter().copied().collect();
    s.insert(IpAddr::V4(Ipv4Addr::LOCALHOST));
    s
}

fn dn(cn: &str) -> DistinguishedName {
    let mut n = DistinguishedName::new();
    n.push(DnType::OrganizationName, ORG);
    n.push(DnType::CommonName, cn);
    n
}

fn ca_params(now: OffsetDateTime) -> CertificateParams {
    let serial = random_serial();
    let suffix: String = serial[12..].iter().map(|b| format!("{b:02x}")).collect();
    let mut p = CertificateParams::default();
    p.distinguished_name = dn(&format!("Tatakai Link CA {suffix}"));
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p.serial_number = Some(SerialNumber::from_slice(&serial));
    p.not_before = whole_seconds(now - BACKDATE);
    p.not_after = whole_seconds(now + CA_VALIDITY);
    p
}

fn issue_server(issuer: &Issuer<'_, KeyPair>, ips: &BTreeSet<IpAddr>, now: OffsetDateTime) -> Result<(Certificate, KeyPair), IdentityError> {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut p = CertificateParams::default();
    p.distinguished_name = dn("Tatakai Link server");
    p.subject_alt_names =
        std::iter::once(SanType::DnsName("localhost".try_into()?)).chain(ips.iter().map(|ip| SanType::IpAddress(*ip))).collect();
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.use_authority_key_identifier_extension = true;
    p.serial_number = Some(SerialNumber::from_slice(&random_serial()));
    p.not_before = whole_seconds(now - BACKDATE);
    p.not_after = whole_seconds(now + SERVER_VALIDITY);
    Ok((p.signed_by(&key, issuer)?, key))
}

/// 128-bit positive serial without leading zero bytes.
fn random_serial() -> Vec<u8> {
    let mut s = uuid::Uuid::new_v4().as_bytes().to_vec();
    s[0] = (s[0] & 0x7f) | 0x40;
    s
}

fn whole_seconds(t: OffsetDateTime) -> OffsetDateTime {
    t - Duration::nanoseconds(i64::from(t.nanosecond()))
}

fn parse<'a>(der: &'a CertificateDer<'_>) -> Option<X509Certificate<'a>> {
    x509_parser::parse_x509_certificate(der).ok().map(|(_, c)| c)
}

fn key_matches(cert: &CertificateDer<'_>, key: &KeyPair) -> bool {
    parse(cert).is_some_and(|c| *c.public_key().subject_public_key.data == *key.public_key_raw())
}

fn san_ips(cert: &X509Certificate<'_>) -> BTreeSet<IpAddr> {
    let Ok(Some(san)) = cert.subject_alternative_name() else { return BTreeSet::new() };
    san.value
        .general_names
        .iter()
        .filter_map(|n| match n {
            GeneralName::IPAddress(o) => match o.len() {
                4 => <[u8; 4]>::try_from(*o).ok().map(IpAddr::from),
                16 => <[u8; 16]>::try_from(*o).ok().map(IpAddr::from),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    fn csr(key: &KeyPair) -> String {
        let mut p = CertificateParams::default();
        p.distinguished_name = dn("phone");
        p.serialize_request(key).unwrap().pem().unwrap()
    }

    #[test]
    fn creates_persists_and_reloads_the_same_ca_and_renews_server_cert_on_ip_change() {
        let dir = tempfile::tempdir().unwrap();
        let ips = ["192.168.1.20".parse().unwrap()];
        let a = Identity::load_or_init(dir.path(), &ips, now()).unwrap();
        let pin = a.ca_fingerprint();
        assert_eq!(pin.len(), 64);
        assert!(a.server_ips().contains(&"192.168.1.20".parse::<IpAddr>().unwrap()));
        assert!(a.server_ips().contains(&"127.0.0.1".parse::<IpAddr>().unwrap()));
        // CA key must not be stored in the clear on Windows
        let raw = fs::read(dir.path().join(CA_KEY_FILE)).unwrap();
        if cfg!(windows) {
            assert!(KeyPair::try_from(raw.as_slice()).is_err());
        }

        let b = Identity::load_or_init(dir.path(), &ips, now()).unwrap();
        assert_eq!(b.ca_fingerprint(), pin);
        assert_eq!(a.server_chain()[0], b.server_chain()[0], "unchanged IPs keep the server cert");

        let c = Identity::load_or_init(dir.path(), &["192.168.1.99".parse().unwrap()], now()).unwrap();
        assert_eq!(c.ca_fingerprint(), pin, "the CA (and so the phone pin) never changes");
        assert!(c.server_ips().contains(&"192.168.1.99".parse::<IpAddr>().unwrap()));
        assert!(!c.server_ips().contains(&"192.168.1.20".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn issues_a_client_certificate_for_the_csr_key_only() {
        let dir = tempfile::tempdir().unwrap();
        let id = Identity::load_or_init(dir.path(), &[], now()).unwrap();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let issued = id.issue_client_from_csr(&csr(&key), now()).unwrap();
        let (_, cert) = x509_parser::parse_x509_certificate(&issued.der).unwrap();
        assert_eq!(*cert.public_key().subject_public_key.data, *key.public_key_raw());
        assert!(cert.subject().to_string().contains(&issued.device_id));
        assert_eq!(issued.fingerprint, sha256_hex(&issued.der));
        let ca = parse(id.ca_der()).unwrap();
        cert.verify_signature(Some(ca.public_key())).unwrap();

        assert!(id.issue_client_from_csr("not a csr", now()).is_err());
        // corrupt the signature (last base64 body line): the CSR self-signature no longer verifies
        let good = csr(&key);
        let mut lines: Vec<String> = good.lines().map(str::to_owned).collect();
        let idx = lines.len() - 2;
        let first = lines[idx].remove(0);
        lines[idx].insert(0, if first == 'A' { 'B' } else { 'A' });
        assert!(id.issue_client_from_csr(&lines.join("
"), now()).is_err());
    }
}
