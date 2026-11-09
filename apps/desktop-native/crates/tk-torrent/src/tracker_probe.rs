//! Tracker status probe (wave A, worker **i**; checklist `torrent.trackers.status`).
//!
//! librqbit 9.1.0 announces to every tracker but keeps the outcome private (`librqbit-tracker-comms` only logs
//! errors at debug level and yields peer addresses). So the engine asks each tracker itself, the way the classic
//! client's tracker table did:
//! * `udp://`: BEP 15 connect + scrape (seeders / completed / leechers for the info-hash).
//! * `http(s)://`: BEP 48 scrape when the path ends in `announce` (`.../scrape?info_hash=`), else an announce with
//!   `numwant=0` (no peers wanted; the answer still carries `complete` / `incomplete` or a `failure reason`).
//! * anything else (`wss://` WebTorrent trackers): not spoken by the engine, reported as failed with that reason.
//!
//! The result is what the Trackers table shows (`TrackerStatus::Working` with counts, or `Failed` with the
//! message). Pure parsing (bencode, UDP packets, URLs) is unit-tested; the network part has a short timeout.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

/// What a tracker said about one info-hash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProbeOk {
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub downloaded: Option<u32>,
    /// `warning message` of an HTTP tracker.
    pub warning: Option<String>,
}

/// Timeout of one UDP exchange / HTTP request.
const TIMEOUT: Duration = Duration::from_secs(6);
/// BEP 15 magic `protocol_id` of the connect request.
const UDP_PROTOCOL_ID: u64 = 0x0417_2710_1980;

/// Ask `url` about `info_hash` (20 raw bytes). `peer_id` / `port` are used by announce fallbacks only.
pub(crate) async fn probe(url: &str, info_hash: [u8; 20], peer_id: [u8; 20], port: u16) -> Result<ProbeOk, String> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("invalid tracker url: {e}"))?;
    match parsed.scheme() {
        "udp" => tokio::time::timeout(TIMEOUT * 2, udp_scrape(&parsed, info_hash))
            .await
            .map_err(|_| "no answer (timed out)".to_owned())?,
        "http" | "https" => http_probe(&parsed, info_hash, peer_id, port).await,
        other => Err(format!("{other}:// trackers are not supported by the torrent engine")),
    }
}

// ---------------------------------------------------------------------------------------------
// UDP (BEP 15)
// ---------------------------------------------------------------------------------------------

fn connect_request(tx: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&UDP_PROTOCOL_ID.to_be_bytes());
    b[8..12].copy_from_slice(&0u32.to_be_bytes());
    b[12..16].copy_from_slice(&tx.to_be_bytes());
    b
}

fn scrape_request(connection_id: u64, tx: u32, info_hash: &[u8; 20]) -> [u8; 36] {
    let mut b = [0u8; 36];
    b[..8].copy_from_slice(&connection_id.to_be_bytes());
    b[8..12].copy_from_slice(&2u32.to_be_bytes());
    b[12..16].copy_from_slice(&tx.to_be_bytes());
    b[16..36].copy_from_slice(info_hash);
    b
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// An `action 3` error packet's message.
fn udp_error(b: &[u8], tx: u32) -> Option<String> {
    (be32(b, 0) == Some(3) && be32(b, 4) == Some(tx)).then(|| String::from_utf8_lossy(&b[8.min(b.len())..]).trim().to_owned())
}

fn parse_connect(b: &[u8], tx: u32) -> Result<u64, String> {
    if let Some(msg) = udp_error(b, tx) {
        return Err(msg);
    }
    if b.len() < 16 || be32(b, 0) != Some(0) || be32(b, 4) != Some(tx) {
        return Err("invalid connect answer".into());
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(&b[8..16]);
    Ok(u64::from_be_bytes(id))
}

fn parse_scrape(b: &[u8], tx: u32) -> Result<ProbeOk, String> {
    if let Some(msg) = udp_error(b, tx) {
        return Err(msg);
    }
    if b.len() < 20 || be32(b, 0) != Some(2) || be32(b, 4) != Some(tx) {
        return Err("invalid scrape answer".into());
    }
    Ok(ProbeOk { seeders: be32(b, 8), downloaded: be32(b, 12), leechers: be32(b, 16), warning: None })
}

async fn exchange(sock: &UdpSocket, req: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; 2048];
    // One retry: UDP drops packets.
    for _ in 0..2 {
        sock.send(req).await.map_err(|e| e.to_string())?;
        match tokio::time::timeout(TIMEOUT / 2, sock.recv(&mut buf)).await {
            Ok(Ok(n)) => return Ok(buf[..n].to_vec()),
            Ok(Err(e)) => return Err(e.to_string()),
            Err(_) => continue,
        }
    }
    Err("no answer (timed out)".into())
}

async fn udp_scrape(url: &url::Url, info_hash: [u8; 20]) -> Result<ProbeOk, String> {
    let host = url.host_str().ok_or("tracker url without a host")?.trim_matches(['[', ']']).to_owned();
    let port = url.port().ok_or("udp tracker url without a port")?;
    let addr: SocketAddr = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("cannot resolve {host}"))?;
    let bind = if addr.is_ipv4() {
        SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0))
    };
    let sock = UdpSocket::bind(bind).await.map_err(|e| e.to_string())?;
    sock.connect(addr).await.map_err(|e| e.to_string())?;
    let tx = rand_u32();
    let conn = parse_connect(&exchange(&sock, &connect_request(tx)).await?, tx)?;
    let tx = rand_u32();
    parse_scrape(&exchange(&sock, &scrape_request(conn, tx, &info_hash)).await?, tx)
}

fn rand_u32() -> u32 {
    let b = uuid::Uuid::new_v4().into_bytes();
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

// ---------------------------------------------------------------------------------------------
// HTTP (BEP 3 / BEP 48)
// ---------------------------------------------------------------------------------------------

/// `%XX` for every byte that is not unreserved (RFC 3986), as trackers expect for `info_hash` / `peer_id`.
fn percent_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// BEP 48: the scrape URL of an announce URL (`.../announce[...]` -> `.../scrape[...]`), `None` when the last path
/// segment does not start with `announce`.
fn scrape_url(announce: &url::Url) -> Option<url::Url> {
    let path = announce.path();
    let (dir, last) = path.rsplit_once('/')?;
    let rest = last.strip_prefix("announce")?;
    let mut u = announce.clone();
    u.set_path(&format!("{dir}/scrape{rest}"));
    Some(u)
}

/// `url` plus raw query pairs (already encoded), keeping its own query.
fn with_query(url: &url::Url, pairs: &[(&str, String)]) -> String {
    let extra: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut s = url.to_string();
    s.push(if url.query().is_some_and(|q| !q.is_empty()) { '&' } else { '?' });
    s.push_str(&extra.join("&"));
    s
}

async fn http_probe(url: &url::Url, info_hash: [u8; 20], peer_id: [u8; 20], port: u16) -> Result<ProbeOk, String> {
    let client = reqwest_rqbit::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("Tatakai/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let hash = percent_bytes(&info_hash);
    let target = match scrape_url(url) {
        Some(s) => with_query(&s, &[("info_hash", hash.clone())]),
        None => with_query(
            url,
            &[
                ("info_hash", hash.clone()),
                ("peer_id", percent_bytes(&peer_id)),
                ("port", port.max(1).to_string()),
                ("uploaded", "0".into()),
                ("downloaded", "0".into()),
                ("left", "0".into()),
                ("numwant", "0".into()),
                ("compact", "1".into()),
            ],
        ),
    };
    let resp = client.get(&target).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("http status {}", status.as_u16()));
    }
    parse_http(&body, &info_hash)
}

/// A scrape (`files` dict keyed by the raw info-hash) or announce answer.
fn parse_http(body: &[u8], info_hash: &[u8; 20]) -> Result<ProbeOk, String> {
    let root = bdecode(body).ok_or("the tracker answer is not bencoded")?;
    if let Some(reason) = root.get(b"failure reason").and_then(Bencode::text) {
        return Err(reason);
    }
    let warning = root.get(b"warning message").and_then(Bencode::text);
    let stats = match root.get(b"files") {
        Some(files) => files.get(info_hash).ok_or("the tracker does not know this torrent")?,
        None => &root,
    };
    Ok(ProbeOk {
        seeders: count(stats, b"complete"),
        leechers: count(stats, b"incomplete"),
        downloaded: count(stats, b"downloaded"),
        warning,
    })
}

fn count(stats: &Bencode, key: &[u8]) -> Option<u32> {
    stats.get(key).and_then(Bencode::int).and_then(|v| u32::try_from(v).ok())
}

// ---------------------------------------------------------------------------------------------
// Minimal bencode reader (tracker answers only)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Bencode {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Bencode>),
    Dict(Vec<(Vec<u8>, Bencode)>),
}

impl Bencode {
    fn get(&self, key: &[u8]) -> Option<&Bencode> {
        match self {
            Self::Dict(items) => items.iter().find(|(k, _)| k.as_slice() == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn text(&self) -> Option<String> {
        match self {
            Self::Bytes(b) => Some(String::from_utf8_lossy(b).trim().to_owned()),
            _ => None,
        }
    }
}

/// Decode one value that spans the whole input (trailing bytes are tolerated).
fn bdecode(input: &[u8]) -> Option<Bencode> {
    let mut pos = 0;
    let v = decode_at(input, &mut pos, 0)?;
    Some(v)
}

fn decode_at(b: &[u8], pos: &mut usize, depth: usize) -> Option<Bencode> {
    if depth > 32 {
        return None;
    }
    match *b.get(*pos)? {
        b'i' => {
            let end = *pos + b[*pos..].iter().position(|c| *c == b'e')?;
            let n = std::str::from_utf8(&b[*pos + 1..end]).ok()?.parse().ok()?;
            *pos = end + 1;
            Some(Bencode::Int(n))
        }
        b'l' => {
            *pos += 1;
            let mut items = Vec::new();
            while *b.get(*pos)? != b'e' {
                items.push(decode_at(b, pos, depth + 1)?);
            }
            *pos += 1;
            Some(Bencode::List(items))
        }
        b'd' => {
            *pos += 1;
            let mut items = Vec::new();
            while *b.get(*pos)? != b'e' {
                let Bencode::Bytes(key) = decode_at(b, pos, depth + 1)? else { return None };
                let value = decode_at(b, pos, depth + 1)?;
                items.push((key, value));
            }
            *pos += 1;
            Some(Bencode::Dict(items))
        }
        b'0'..=b'9' => {
            let colon = *pos + b[*pos..].iter().position(|c| *c == b':')?;
            let len: usize = std::str::from_utf8(&b[*pos..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            let end = start.checked_add(len)?;
            let bytes = b.get(start..end)?.to_vec();
            *pos = end;
            Some(Bencode::Bytes(bytes))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_packets() {
        let c = connect_request(7);
        assert_eq!(&c[..8], &0x0417_2710_1980u64.to_be_bytes());
        assert_eq!(be32(&c, 12), Some(7));
        let mut ans = vec![0, 0, 0, 0, 0, 0, 0, 7];
        ans.extend_from_slice(&42u64.to_be_bytes());
        assert_eq!(parse_connect(&ans, 7), Ok(42));
        assert!(parse_connect(&ans, 8).is_err());
        let s = scrape_request(42, 9, &[1u8; 20]);
        assert_eq!(be32(&s, 8), Some(2));
        assert_eq!(&s[16..], &[1u8; 20]);
        let mut sc = vec![0, 0, 0, 2, 0, 0, 0, 9];
        for v in [12u32, 300, 4] {
            sc.extend_from_slice(&v.to_be_bytes());
        }
        assert_eq!(parse_scrape(&sc, 9), Ok(ProbeOk { seeders: Some(12), downloaded: Some(300), leechers: Some(4), warning: None }));
        let mut err = vec![0, 0, 0, 3, 0, 0, 0, 9];
        err.extend_from_slice(b"torrent not registered");
        assert_eq!(parse_scrape(&err, 9), Err("torrent not registered".to_owned()));
    }

    #[test]
    fn http_urls() {
        let a = url::Url::parse("http://nyaa.tracker.wf:7777/announce").unwrap();
        assert_eq!(scrape_url(&a).unwrap().as_str(), "http://nyaa.tracker.wf:7777/scrape");
        let b = url::Url::parse("https://t.example/x/announce.php?passkey=1").unwrap();
        assert_eq!(scrape_url(&b).unwrap().as_str(), "https://t.example/x/scrape.php?passkey=1");
        assert!(scrape_url(&url::Url::parse("http://t.example/a").unwrap()).is_none());
        assert_eq!(percent_bytes(&[0x12, b'a', 0xff, b'~']), "%12a%FF~");
        let q = with_query(&b, &[("info_hash", "%12".into())]);
        assert!(q.ends_with("passkey=1&info_hash=%12"));
    }

    #[test]
    fn bencode_answers() {
        let hash = [b'h'; 20];
        let mut scrape = b"d5:filesd20:".to_vec();
        scrape.extend_from_slice(&hash);
        scrape.extend_from_slice(b"d8:completei5e10:downloadedi50e10:incompletei10eeee");
        assert_eq!(
            parse_http(&scrape, &hash),
            Ok(ProbeOk { seeders: Some(5), leechers: Some(10), downloaded: Some(50), warning: None })
        );
        assert_eq!(parse_http(b"d14:failure reason11:unregisterede", &hash), Err("unregistered".to_owned()));
        let announce = b"d8:completei3e10:incompletei1e8:intervali1800e5:peers0:e";
        assert_eq!(parse_http(announce, &hash).unwrap().seeders, Some(3));
        assert!(parse_http(b"<html>", &hash).is_err());
        assert_eq!(bdecode(b"l1:ai-2ee"), Some(Bencode::List(vec![Bencode::Bytes(b"a".to_vec()), Bencode::Int(-2)])));
        assert_eq!(bdecode(b"5:ab"), None);
    }
}
