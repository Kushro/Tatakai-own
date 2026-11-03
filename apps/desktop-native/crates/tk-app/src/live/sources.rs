//! Source selector glue (docs/redesign/08 sections 4 and 7): raw extension streams -> tk-core
//! `SourceCandidate` -> ranked `SourceVm`. Pure functions; the backend adds the store and the cache.

use tk_core::StreamSource;
use tk_core::sources::{Quality, SourceCandidate, SourceKind, SourceMode, SourcePrefs, rank, source_key};
use tk_ui::backend::{SourceKindVm, SourceVm, StreamVm, SubtitleVm};
use tk_ui::model::{QualityVm, SourceModeVm, SourcePrefsVm};

pub fn core_kind(k: SourceKindVm) -> SourceKind {
    match k {
        SourceKindVm::Web => SourceKind::Web,
        SourceKindVm::Torrent => SourceKind::Torrent,
    }
}

pub fn kind_vm(k: SourceKind) -> SourceKindVm {
    match k {
        SourceKind::Web => SourceKindVm::Web,
        SourceKind::Torrent => SourceKindVm::Torrent,
    }
}

/// `Settings::sources` -> the prefs tk-core ranks with.
pub fn core_prefs(p: &SourcePrefsVm) -> SourcePrefs {
    SourcePrefs {
        mode: match p.mode {
            SourceModeVm::Auto => SourceMode::Auto,
            SourceModeVm::Manual => SourceMode::Manual,
        },
        prefer: p.prefer.map(core_kind),
        min_quality: match p.min_quality {
            QualityVm::P480 => Quality::P480,
            QualityVm::P720 => Quality::P720,
            QualityVm::P1080 => Quality::P1080,
            QualityVm::P1440 => Quality::P1440,
            QualityVm::P2160 => Quality::P2160,
        },
        sub_lang: p.sub_lang.clone(),
        dub_lang: p.dub_lang.clone(),
    }
}

/// Wrap every stream as a candidate. `source_key` is `<ext>|<kind>|<label>`, so two streams of the same server
/// (one extension returning 1080p and 720p of "Alpha") would share an id: keep the best quality, first wins on ties.
pub fn candidates_from_streams(streams: &[StreamSource], ext_name: impl Fn(&str) -> String) -> Vec<SourceCandidate> {
    let mut out: Vec<(String, SourceCandidate)> = Vec::new();
    for s in streams {
        let c = SourceCandidate::from_stream(s, &ext_name(&s.extension_id));
        let key = source_key(&c);
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, prev)) if c.quality > prev.quality => *prev = c,
            Some(_) => {}
            None => out.push((key, c)),
        }
    }
    out.into_iter().map(|(_, c)| c).collect()
}

fn source_vm(c: SourceCandidate, score: f32, ext_enabled: bool, picked: Option<&str>, recommended: bool) -> SourceVm {
    let id = source_key(&c);
    let picked = picked == Some(id.as_str());
    let (latency_ms, size_bytes, seeders, leechers, info_hash) = (
        c.web.as_ref().and_then(|w| w.latency_ms),
        c.torrent.as_ref().and_then(|t| t.size_bytes),
        c.torrent.as_ref().map(|t| t.seeders),
        c.torrent.as_ref().map(|t| t.leechers),
        c.torrent.as_ref().map(|t| t.info_hash.clone()),
    );
    SourceVm {
        id,
        ext_id: c.ext_id,
        ext_name: c.ext_name,
        kind: kind_vm(c.kind),
        label: c.label,
        quality: c.quality.label().to_owned(),
        container: c.container,
        subs: c.subs,
        dubs: c.dubs,
        reliability: c.reliability,
        note: c.note,
        latency_ms,
        size_bytes,
        seeders,
        leechers,
        info_hash,
        ext_enabled,
        score,
        recommended,
        picked,
    }
}

/// Rank with tk-core and map to view-models: `recommended` on `[0]`, `picked` on the stored manual pick.
pub fn ranked_vms(
    candidates: Vec<SourceCandidate>,
    prefs: &SourcePrefs,
    ext_enabled: impl Fn(&str) -> bool,
    picked: Option<&str>,
) -> Vec<SourceVm> {
    rank(candidates, prefs, &ext_enabled)
        .into_iter()
        .enumerate()
        .map(|(i, (c, score))| {
            let enabled = ext_enabled(&c.ext_id);
            source_vm(c, score, enabled, picked, i == 0)
        })
        .collect()
}

pub fn stream_vm(s: StreamSource) -> StreamVm {
    StreamVm {
        url: s.url,
        headers: s.headers.into_iter().collect(),
        provider: s.provider,
        quality: s.quality,
        subtitles: s.subtitles.into_iter().map(|t| SubtitleVm { url: t.url, lang: t.lang, label: t.label }).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::sources::{TorrentInfo, WebInfo};

    fn web(ext: &str, label: &str, q: Quality) -> SourceCandidate {
        SourceCandidate {
            ext_id: ext.into(),
            ext_name: ext.to_uppercase(),
            kind: SourceKind::Web,
            label: label.into(),
            quality: q,
            container: Some("HLS".into()),
            subs: vec!["es".into()],
            dubs: vec!["ja".into()],
            reliability: 0.9,
            note: None,
            web: Some(WebInfo { url: format!("https://{label}.test/a.m3u8"), headers: Default::default(), latency_ms: Some(120), subtitles: Vec::new() }),
            torrent: None,
        }
    }

    #[test]
    fn prefs_map_one_to_one() {
        let p = SourcePrefsVm { mode: SourceModeVm::Manual, prefer: Some(SourceKindVm::Torrent), min_quality: QualityVm::P1080, sub_lang: "en".into(), dub_lang: "es".into() };
        let c = core_prefs(&p);
        assert_eq!(c.mode, SourceMode::Manual);
        assert_eq!(c.prefer, Some(SourceKind::Torrent));
        assert_eq!(c.min_quality, Quality::P1080);
        assert_eq!((c.sub_lang.as_str(), c.dub_lang.as_str()), ("en", "es"));
        assert_eq!(core_prefs(&SourcePrefsVm::default()), SourcePrefs::default());
    }

    #[test]
    fn ranked_list_marks_recommended_and_picked() {
        let mut torrent = web("nyaa", "SubsPlease", Quality::P1080);
        torrent.kind = SourceKind::Torrent;
        torrent.web = None;
        torrent.torrent = Some(TorrentInfo { info_hash: "ab".repeat(20), size_bytes: Some(1 << 30), seeders: 300, leechers: 12, magnet: None });
        let list = vec![web("a", "Alpha", Quality::P480), web("a", "Beta", Quality::P1080), torrent];
        let picked = source_key(&list[0]);
        let vms = ranked_vms(list, &SourcePrefs::default(), |id| id != "nyaa", Some(&picked));
        assert_eq!(vms.len(), 3);
        assert!(vms[0].recommended && vms.iter().skip(1).all(|v| !v.recommended));
        assert_eq!(vms.iter().filter(|v| v.picked).count(), 1);
        assert_eq!(vms.iter().find(|v| v.picked).unwrap().label, "Alpha");
        let t = vms.iter().find(|v| v.kind == SourceKindVm::Torrent).unwrap();
        assert!(!t.ext_enabled);
        assert_eq!((t.seeders, t.info_hash.as_ref().map(String::len)), (Some(300), Some(40)));
        assert!(vms.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(vms[0].id, format!("{}|{}|{}", vms[0].ext_id, vms[0].kind.id(), vms[0].label));
    }
}
