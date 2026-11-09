//! HLS playlist helpers (worker **h**): master -> variant choice by quality, media playlist -> segment plan with
//! absolute URLs, byte ranges, the `#EXT-X-MAP` init segment and `#EXT-X-KEY` AES-128 keys (decrypted per segment
//! with the RustCrypto `aes` + `cbc` crates, `NOTES-deps.md`). `SAMPLE-AES` and DRM key formats are reported as
//! `DownloadError::Playlist("encrypted")`. Parsing with `m3u8_rs`. Pure functions (tested); the network side lives
//! in `engine.rs`.

use m3u8_rs::{KeyMethod, MasterPlaylist, MediaPlaylist, Playlist, VariantStream};
use url::Url;

use crate::error::DownloadError;

/// Resolve a segment / variant URI against the playlist URL (pure; tested).
pub fn absolute(base: &str, uri: &str) -> Result<String, DownloadError> {
    let base = Url::parse(base).map_err(|e| DownloadError::Playlist(e.to_string()))?;
    base.join(uri).map(|u| u.to_string()).map_err(|e| DownloadError::Playlist(e.to_string()))
}

/// Whether a body looks like a playlist (`#EXTM3U` after an optional BOM / whitespace).
pub fn is_playlist(body: &[u8]) -> bool {
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
    let start = body.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(body.len());
    body[start..].starts_with(b"#EXTM3U")
}

/// Parsed playlist: a master (variants) or a media playlist (segments).
pub enum Parsed {
    Master(MasterPlaylist),
    Media(MediaPlaylist),
}

pub fn parse(body: &[u8]) -> Result<Parsed, DownloadError> {
    match m3u8_rs::parse_playlist_res(body) {
        Ok(Playlist::MasterPlaylist(m)) => Ok(Parsed::Master(m)),
        Ok(Playlist::MediaPlaylist(m)) => Ok(Parsed::Media(m)),
        Err(e) => Err(DownloadError::Playlist(format!("invalid playlist: {e:?}").chars().take(200).collect())),
    }
}

/// Wanted vertical resolution of a quality label (`"1080p"` -> 1080); `None` = the best available
/// (`"best"`, empty, unknown).
pub fn wanted_height(quality: Option<&str>) -> Option<u64> {
    let q = quality?.trim().to_ascii_lowercase();
    let digits: String = q.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<u64>() {
        Ok(h) if h >= 144 => Some(h),
        _ if q == "4k" => Some(2160),
        _ => None,
    }
}

/// Height of a variant: `RESOLUTION`, else a guess from the bandwidth (so audio-less ladders still sort).
fn variant_height(v: &VariantStream) -> u64 {
    match &v.resolution {
        Some(r) => r.height,
        None => match v.bandwidth {
            b if b >= 6_000_000 => 1080,
            b if b >= 2_500_000 => 720,
            b if b >= 1_000_000 => 480,
            _ => 360,
        },
    }
}

/// Index of the variant to download: with `want`, the tallest one not above it (else the smallest); without, the
/// tallest. Ties go to the higher bandwidth. I-frame-only variants are never picked. `None` when there is none.
pub fn pick_variant(variants: &[VariantStream], want: Option<u64>) -> Option<usize> {
    let usable: Vec<(usize, u64, u64)> =
        variants.iter().enumerate().filter(|(_, v)| !v.is_i_frame).map(|(i, v)| (i, variant_height(v), v.bandwidth)).collect();
    if usable.is_empty() {
        return None;
    }
    let tallest = |limit: u64| usable.iter().filter(|t| t.1 <= limit).max_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2))).map(|t| t.0);
    match want {
        Some(h) => tallest(h).or_else(|| usable.iter().min_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2))).map(|t| t.0)),
        None => tallest(u64::MAX),
    }
}

/// AES-128 key of a segment: key URL and the 16-byte IV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    pub uri: String,
    pub iv: [u8; 16],
}

/// One segment to fetch, in playlist order.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentPlan {
    pub url: String,
    /// `(length, offset)` for `#EXT-X-BYTERANGE`.
    pub range: Option<(u64, u64)>,
    pub key: Option<KeySpec>,
    pub duration: f32,
}

/// The whole media playlist as a download plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// `#EXT-X-MAP` init segment (fMP4), written once before the first segment.
    pub init: Option<SegmentPlan>,
    pub segments: Vec<SegmentPlan>,
    /// fMP4 segments (an init map): the output is `.mp4` instead of `.ts`.
    pub fmp4: bool,
}

/// `0x...` IV attribute -> 16 bytes (left-padded).
pub fn parse_iv(text: &str) -> Option<[u8; 16]> {
    let hex_part = text.trim().strip_prefix("0x").or_else(|| text.trim().strip_prefix("0X"))?;
    if hex_part.is_empty() || hex_part.len() > 32 {
        return None;
    }
    let padded = format!("{hex_part:0>32}");
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Default IV of a segment: its media sequence number, big-endian.
pub fn sequence_iv(sequence: u64) -> [u8; 16] {
    u128::from(sequence).to_be_bytes()
}

/// Build the plan of a media playlist fetched from `base`. Keys apply to every following segment until the next
/// `#EXT-X-KEY`; byte ranges without an offset continue after the previous range of the same resource.
pub fn plan(base: &str, pl: &MediaPlaylist) -> Result<Plan, DownloadError> {
    let mut out = Plan::default();
    let mut key: Option<(String, Option<[u8; 16]>)> = None;
    let mut last_end: Option<(String, u64)> = None;
    for (i, seg) in pl.segments.iter().enumerate() {
        if let Some(k) = &seg.key {
            key = match &k.method {
                KeyMethod::None => None,
                KeyMethod::AES128 => {
                    let uri = k.uri.as_deref().ok_or_else(|| DownloadError::Playlist("key without uri".into()))?;
                    if k.keyformat.as_deref().is_some_and(|f| f != "identity") {
                        return Err(DownloadError::Playlist("encrypted".into()));
                    }
                    let iv = match k.iv.as_deref() {
                        Some(t) => Some(parse_iv(t).ok_or_else(|| DownloadError::Playlist("bad iv".into()))?),
                        None => None,
                    };
                    Some((absolute(base, uri)?, iv))
                }
                KeyMethod::SampleAES | KeyMethod::Other(_) => return Err(DownloadError::Playlist("encrypted".into())),
            };
        }
        if out.init.is_none()
            && let Some(map) = &seg.map
        {
            let url = absolute(base, &map.uri)?;
            let range = map.byte_range.as_ref().map(|r| (r.length, r.offset.unwrap_or(0)));
            out.init = Some(SegmentPlan { url, range, key: None, duration: 0.0 });
            out.fmp4 = true;
        }
        let url = absolute(base, &seg.uri)?;
        let range = seg.byte_range.as_ref().map(|r| {
            let offset = match r.offset {
                Some(o) => o,
                None => match &last_end {
                    Some((u, end)) if *u == url => *end,
                    _ => 0,
                },
            };
            (r.length, offset)
        });
        if let Some((len, off)) = range {
            last_end = Some((url.clone(), off + len));
        }
        let seg_key = key.as_ref().map(|(uri, iv)| KeySpec {
            uri: uri.clone(),
            iv: iv.unwrap_or_else(|| sequence_iv(pl.media_sequence + i as u64)),
        });
        out.segments.push(SegmentPlan { url, range, key: seg_key, duration: seg.duration });
    }
    if out.segments.is_empty() {
        return Err(DownloadError::Playlist("empty playlist".into()));
    }
    Ok(out)
}

/// Decrypt one AES-128-CBC segment in place (PKCS#7 padding, as HLS specifies).
pub fn decrypt_aes128(data: &mut Vec<u8>, key: &[u8; 16], iv: &[u8; 16]) -> Result<(), DownloadError> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
    type Dec = cbc::Decryptor<aes::Aes128>;
    let len = Dec::new(&(*key).into(), &(*iv).into())
        .decrypt_padded_mut::<Pkcs7>(data.as_mut_slice())
        .map_err(|_| DownloadError::Playlist("segment decryption failed".into()))?
        .len();
    data.truncate(len);
    Ok(())
}

/// `Range` header value of a byte range.
pub fn range_header((len, off): (u64, u64)) -> String {
    format!("bytes={off}-{}", off + len.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_absolute_uris() {
        assert_eq!(absolute("https://h/a/master.m3u8", "seg1.ts").unwrap(), "https://h/a/seg1.ts");
        assert_eq!(absolute("https://h/a/master.m3u8", "/root/seg1.ts").unwrap(), "https://h/root/seg1.ts");
        assert_eq!(absolute("https://h/a/master.m3u8", "https://cdn/x.ts").unwrap(), "https://cdn/x.ts");
    }

    #[test]
    fn detects_playlists() {
        assert!(is_playlist(b"#EXTM3U\n#EXT-X-VERSION:3"));
        assert!(is_playlist(b"\xEF\xBB\xBF  \n#EXTM3U"));
        assert!(!is_playlist(b"\x00\x00\x00\x18ftypmp42"));
    }

    const MASTER: &str = "#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
360.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080
1080.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1280x720
720.m3u8
";

    fn variants() -> Vec<VariantStream> {
        match parse(MASTER.as_bytes()).unwrap() {
            Parsed::Master(m) => m.variants,
            Parsed::Media(_) => panic!("master expected"),
        }
    }

    #[test]
    fn variant_by_quality() {
        let v = variants();
        assert_eq!(v[pick_variant(&v, None).unwrap()].uri, "1080.m3u8");
        assert_eq!(v[pick_variant(&v, Some(720)).unwrap()].uri, "720.m3u8");
        assert_eq!(v[pick_variant(&v, Some(1000)).unwrap()].uri, "720.m3u8");
        // nothing small enough: the smallest
        assert_eq!(v[pick_variant(&v, Some(240)).unwrap()].uri, "360.m3u8");
        assert_eq!(pick_variant(&[], None), None);
    }

    #[test]
    fn quality_labels() {
        assert_eq!(wanted_height(Some("1080p")), Some(1080));
        assert_eq!(wanted_height(Some("720")), Some(720));
        assert_eq!(wanted_height(Some("4K")), Some(2160));
        assert_eq!(wanted_height(Some("best")), None);
        assert_eq!(wanted_height(None), None);
    }

    #[test]
    fn ivs() {
        assert_eq!(parse_iv("0x1").unwrap()[15], 1);
        assert_eq!(parse_iv("0x000102030405060708090a0b0c0d0e0f").unwrap()[1], 1);
        assert!(parse_iv("12").is_none());
        assert_eq!(sequence_iv(258)[14..], [1, 2]);
    }

    const MEDIA: &str = "#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:7
#EXTINF:10.0,
s0.ts
#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\"
#EXTINF:10.0,
s1.ts
#EXT-X-KEY:METHOD=AES-128,URI=\"k2.bin\",IV=0x0000000000000000000000000000000a
#EXTINF:5.0,
https://cdn/s2.ts
#EXT-X-ENDLIST
";

    #[test]
    fn media_plan_with_keys() {
        let Parsed::Media(m) = parse(MEDIA.as_bytes()).unwrap() else { panic!("media expected") };
        let p = plan("https://h/v/index.m3u8", &m).unwrap();
        assert_eq!(p.segments.len(), 3);
        assert!(!p.fmp4);
        assert_eq!(p.segments[0].url, "https://h/v/s0.ts");
        assert!(p.segments[0].key.is_none());
        let k1 = p.segments[1].key.clone().unwrap();
        assert_eq!(k1.uri, "https://h/v/k.bin");
        assert_eq!(k1.iv, sequence_iv(8));
        let k2 = p.segments[2].key.clone().unwrap();
        assert_eq!(k2.uri, "https://h/v/k2.bin");
        assert_eq!(k2.iv[15], 10);
        assert_eq!(p.segments[2].url, "https://cdn/s2.ts");
    }

    #[test]
    fn sample_aes_is_refused() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:4,\na.ts\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        assert!(matches!(plan("https://h/i.m3u8", &m), Err(DownloadError::Playlist(s)) if s == "encrypted"));
    }

    #[test]
    fn fmp4_map_and_byte_ranges() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXT-X-BYTERANGE:100@0\n#EXTINF:4,\nall.m4s\n#EXT-X-BYTERANGE:50\n#EXTINF:4,\nall.m4s\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        let p = plan("https://h/i.m3u8", &m).unwrap();
        assert!(p.fmp4);
        assert_eq!(p.init.unwrap().url, "https://h/init.mp4");
        assert_eq!(p.segments[0].range, Some((100, 0)));
        assert_eq!(p.segments[1].range, Some((50, 100)));
        assert_eq!(range_header((50, 100)), "bytes=100-149");
    }

    #[test]
    fn aes_cbc_vector() {
        // RustCrypto `cbc` documentation vector.
        let key = [0x42u8; 16];
        let iv = [0x24u8; 16];
        let mut data = hex::decode(
            "c7fe247ef97b21f07cbdd26cb5d346bfd27867cb00d9486723e159978fb9a5f914cfb228a710de4171e396e7b6cf859e",
        )
        .unwrap();
        decrypt_aes128(&mut data, &key, &iv).unwrap();
        assert_eq!(data, b"hello world! this is my plaintext.");
        let mut bad = vec![1u8; 15];
        assert!(decrypt_aes128(&mut bad, &key, &iv).is_err());
    }
    // ---- F6 (parity wave B): gaps not covered above ----

    #[test]
    fn i_frame_variants_are_never_picked_and_bandwidth_guesses_the_height() {
        let v = vec![
            VariantStream { is_i_frame: true, uri: "iframe.m3u8".into(), bandwidth: 9_000_000, ..Default::default() },
            VariantStream { uri: "low.m3u8".into(), bandwidth: 900_000, ..Default::default() },
            VariantStream { uri: "mid.m3u8".into(), bandwidth: 3_000_000, ..Default::default() },
            VariantStream { uri: "high.m3u8".into(), bandwidth: 7_000_000, ..Default::default() },
        ];
        assert_eq!(v[pick_variant(&v, None).unwrap()].uri, "high.m3u8");
        // 3 Mb/s guesses 720 lines
        assert_eq!(v[pick_variant(&v, Some(720)).unwrap()].uri, "mid.m3u8");
        assert_eq!(v[pick_variant(&v, Some(480)).unwrap()].uri, "low.m3u8");
        // only I-frame variants: nothing to download
        assert_eq!(pick_variant(&v[..1], None), None);
    }

    #[test]
    fn same_height_ties_go_to_the_higher_bandwidth() {
        let v = vec![
            VariantStream { uri: "a.m3u8".into(), bandwidth: 4_000_000, resolution: Some(m3u8_rs::Resolution { width: 1920, height: 1080 }), ..Default::default() },
            VariantStream { uri: "b.m3u8".into(), bandwidth: 6_000_000, resolution: Some(m3u8_rs::Resolution { width: 1920, height: 1080 }), ..Default::default() },
        ];
        assert_eq!(v[pick_variant(&v, Some(1080)).unwrap()].uri, "b.m3u8");
        assert_eq!(v[pick_variant(&v, None).unwrap()].uri, "b.m3u8");
    }

    #[test]
    fn tiny_or_textual_quality_labels_mean_best() {
        assert_eq!(wanted_height(Some("")), None);
        assert_eq!(wanted_height(Some("  720P ")), Some(720));
        assert_eq!(wanted_height(Some("100")), None); // below 144 is not a height
        assert_eq!(wanted_height(Some("auto")), None);
    }

    #[test]
    fn iv_bounds() {
        assert!(parse_iv("0x").is_none());
        assert!(parse_iv(&format!("0x{}", "0".repeat(33))).is_none());
        assert!(parse_iv("0xzz").is_none());
        assert_eq!(parse_iv(" 0XFF ").unwrap()[15], 0xff);
        assert_eq!(sequence_iv(0), [0u8; 16]);
    }

    #[test]
    fn method_none_clears_the_key_for_following_segments() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\"\n#EXTINF:4,\na.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:4,\nb.ts\n#EXTINF:4,\nc.ts\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        let p = plan("https://h/i.m3u8", &m).unwrap();
        assert_eq!(p.segments.len(), 3);
        assert!(p.segments[0].key.is_some());
        assert!(p.segments[1].key.is_none());
        assert!(p.segments[2].key.is_none());
        assert_eq!(p.segments[0].duration, 4.0);
    }

    #[test]
    fn drm_key_formats_are_refused() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:4,\na.ts\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        assert!(matches!(plan("https://h/i.m3u8", &m), Err(DownloadError::Playlist(s)) if s == "encrypted"));
        // identity is plain AES-128
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.bin\",KEYFORMAT=\"identity\"\n#EXTINF:4,\na.ts\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        assert_eq!(plan("https://h/i.m3u8", &m).unwrap().segments[0].key.as_ref().unwrap().uri, "https://h/k.bin");
    }

    #[test]
    fn empty_media_playlist_is_an_error() {
        assert!(matches!(plan("https://h/i.m3u8", &MediaPlaylist::default()), Err(DownloadError::Playlist(s)) if s == "empty playlist"));
    }

    #[test]
    fn byte_range_offset_restarts_on_another_resource() {
        let text = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-BYTERANGE:100@0\n#EXTINF:4,\na.m4s\n#EXT-X-BYTERANGE:40\n#EXTINF:4,\nb.m4s\n";
        let Parsed::Media(m) = parse(text.as_bytes()).unwrap() else { panic!("media expected") };
        let p = plan("https://h/i.m3u8", &m).unwrap();
        assert_eq!(p.segments[1].range, Some((40, 0)));
        assert!(!p.fmp4 && p.init.is_none());
    }

    #[test]
    fn garbage_is_not_a_playlist() {
        assert!(!is_playlist(b""));
        assert!(!is_playlist(b"   "));
        assert!(matches!(parse(b"<html>nope</html>"), Err(DownloadError::Playlist(_))));
        assert!(absolute("not a url", "a.ts").is_err());
    }
}
