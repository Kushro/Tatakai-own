//! Subtitles in the player (wave A, worker **d**; docs/redesign/15-parity-wave-a-contract.md section 4). Owned by
//! d: preferred subtitle / audio language on `PlayerEvent::Tracks` (the audit bug "preferred subtitle language
//! ignored"), secondary subtitle, style (`Settings::subs.style` -> `tk_player::SubStyle`), delay, external SRT /
//! ASS / SSA / VTT files, the app fonts folder for libass, live and pre-translation (`Backend::translate_cues`) and
//! the screenshot button (`Backend::screenshot_path` + `Player::screenshot_to_file`).
//!
//! `mod.rs` (worker e) calls the three contract seams ([`on_tracks`], [`capture`], [`SubtitleSheet`]) with their
//! exact signatures. [`SubtitleSheet`] is mounted for the whole life of the player screen (it renders nothing but
//! the live-translation overlay while closed), so it also hosts the per-screen work: it polls mpv for the real
//! selection (tracks, delay, secondary), runs the translation jobs and mirrors the state the verifier reads onto
//! `.tk-pc` as `data-*` attributes (`data-sub-lang`, `data-audio-lang`, `data-secondary-lang`, `data-sub-delay-ms`,
//! `data-sub-scale`, `data-translated`, `data-last-capture`), because `.tk-pc` itself is rendered by e's
//! `PlayerControls`.
//!
//! For the integrator (mod.rs is e's): the track menus call [`select_sub`] / [`select_audio`], and an effect on
//! [`TRACKS_REV`] re-reads the track list with [`read_tracks`] after this module adds a track (loaded file,
//! translation), since tk-player only reports `PlayerEvent::Tracks` when a file loads.

mod cues;
mod fonts;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::prelude::*;
use tk_player::{Player, SubStyle, SubtitleSource, TrackInfo, TrackKind};

use super::flow::Flow;
use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, CueVm, TranslateRequestVm};
use crate::components::form::use_uncontrolled_input;
use crate::components::player_controls::track_label;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Sheet, Switch, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::{
    ErrorKind, SettingsAction, SourcePrefsVm, SubtitlePrefs, SubtitleStyleVm, TranslationProviderVm, UiError, reduce_settings,
};

pub(crate) use self::fonts::app_fonts_dir;

/// Bumped whenever this module changes mpv's track list or selection (file loaded, translation added, track picked
/// in the sheet). `mod.rs` re-reads the list with [`read_tracks`] (integration note).
pub static TRACKS_REV: GlobalSignal<u64> = Signal::global(|| 0);
/// Path of the last screenshot (`data-last-capture`).
static LAST_CAPTURE: GlobalSignal<Option<String>> = Signal::global(|| None);
/// `<data>/fonts`, handed to libass as `sub-fonts-dir` on every file (set when the player screen mounts).
static FONTS_DIR: Mutex<Option<String>> = Mutex::new(None);
/// Installed font families, scanned once per run (Settings "Rescan" forces another scan).
static FONT_CACHE: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// How often the sheet reads mpv's selection / delay and mirrors it onto `.tk-pc`.
const POLL_MS: u64 = 400;
/// Live translation tick (the visible cue is looked up this often).
const LIVE_TICK_MS: u64 = 200;
/// Cues translated ahead of the playhead in one live request.
const LIVE_AHEAD: usize = 25;
/// Cues per `translate_cues` call while pre-translating (progress granularity).
const PRE_CHUNK: usize = 60;
/// Auto pre-translation pauses playback only when it starts this close to the beginning.
const AUTO_PAUSE_WITHIN_SECS: f64 = 15.0;
/// Delay step of the -/+ buttons.
const DELAY_STEP_MS: i64 = 100;

fn bump_tracks() {
    *TRACKS_REV.write() += 1;
}

/// Remember the app fonts folder for libass (created if missing).
pub(crate) fn remember_fonts_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(mut g) = FONTS_DIR.lock() {
        *g = Some(dir.to_string_lossy().into_owned());
    }
}

/// Hand the app fonts folder to libass (`sub-fonts-dir`), when known.
fn apply_fonts_dir(p: &Player) {
    let fonts_dir = FONTS_DIR.lock().ok().and_then(|g| g.clone());
    if let Some(dir) = fonts_dir
        && let Err(e) = p.set_option_str("sub-fonts-dir", &dir)
    {
        tracing::debug!(error = %e, "mpv refused sub-fonts-dir");
    }
}

/// `Option<Arc<Player>>` as a component prop (`Player` has no `PartialEq`; identity is what matters).
#[derive(Clone, Default)]
pub struct PlayerHandle(pub Option<Arc<Player>>);

impl PartialEq for PlayerHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Languages and track choice
// ---------------------------------------------------------------------------------------------

/// ISO 639-1 code of a language tag or name (`es`, `spa`, `es-419`, `Spanish`, `Español`, `Latino` ...).
pub fn lang_code(raw: &str) -> Option<&'static str> {
    let lower = raw.trim().to_lowercase();
    let head = lower.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty())?;
    word_lang(head)
}

fn word_lang(w: &str) -> Option<&'static str> {
    Some(match w {
        "es" | "spa" | "esp" | "spanish" | "español" | "espanol" | "castellano" | "latino" | "lat" | "esla" => "es",
        "en" | "eng" | "english" | "inglés" | "ingles" => "en",
        "pt" | "por" | "pob" | "ptbr" | "portuguese" | "português" | "portugues" | "brazilian" => "pt",
        "ja" | "jpn" | "jap" | "japanese" | "japonés" | "japones" | "日本語" => "ja",
        "fr" | "fre" | "fra" | "french" | "français" | "francais" | "francés" => "fr",
        "de" | "ger" | "deu" | "german" | "deutsch" | "alemán" => "de",
        "it" | "ita" | "italian" | "italiano" => "it",
        "ru" | "rus" | "russian" | "русский" => "ru",
        "ar" | "ara" | "arabic" => "ar",
        "zh" | "chi" | "zho" | "chinese" => "zh",
        "ko" | "kor" | "korean" => "ko",
        "pl" | "pol" | "polish" => "pl",
        _ => return None,
    })
}

/// Language of a track: its `lang` tag, else a language word in its title ("Latin American Spanish").
pub fn track_lang(tr: &TrackInfo) -> Option<&'static str> {
    if let Some(code) = tr.lang.as_deref().and_then(lang_code) {
        return Some(code);
    }
    let title = tr.title.as_deref()?.to_lowercase();
    title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).find_map(word_lang)
}

/// Signs / songs / forced tracks only carry a few lines: a full dialogue track of the same language wins.
fn is_partial(tr: &TrackInfo) -> bool {
    let title = tr.title.as_deref().unwrap_or("").to_lowercase();
    ["sign", "song", "forced", "cartel", "letrero", "karaoke", "op/ed"].iter().any(|w| title.contains(w))
}

/// Best track of `kind` in language `want` (ISO 639-1), skipping `exclude`.
fn best_track(list: &[TrackInfo], kind: TrackKind, want: &str, exclude: Option<i64>) -> Option<i64> {
    let want = lang_code(want)?;
    list.iter()
        .filter(|tr| tr.kind == kind && Some(tr.id) != exclude && track_lang(tr) == Some(want))
        .max_by_key(|tr| (!is_partial(tr), tr.selected, std::cmp::Reverse(tr.id)))
        .map(|tr| tr.id)
}

/// The id mpv reports as selected for `kind` (0 = none).
pub fn selected_id(list: &[TrackInfo], kind: TrackKind) -> i64 {
    list.iter().find(|tr| tr.kind == kind && tr.selected).map_or(0, |tr| tr.id)
}

/// Tracks to switch to (`None` = keep mpv's choice).
#[derive(Debug, Default, PartialEq, Eq)]
struct Choice {
    sub: Option<i64>,
    audio: Option<i64>,
    secondary: Option<i64>,
}

/// The preference rules (pure): subtitle in `sources.sub_lang`, audio in `sources.dub_lang`, secondary subtitle in
/// `subs.secondary_lang` (never the same track as the primary). A preference without a matching track leaves
/// mpv's own choice.
fn choose(list: &[TrackInfo], subs: &SubtitlePrefs, sources: &SourcePrefsVm) -> Choice {
    let sub = best_track(list, TrackKind::Sub, &sources.sub_lang, None);
    let audio = best_track(list, TrackKind::Audio, &sources.dub_lang, None);
    let primary = sub.or_else(|| Some(selected_id(list, TrackKind::Sub)).filter(|id| *id != 0));
    let secondary = subs.secondary_lang.as_deref().and_then(|l| best_track(list, TrackKind::Sub, l, primary));
    Choice { sub, audio, secondary }
}

/// mpv style options from the saved style.
pub fn to_sub_style(s: &SubtitleStyleVm) -> SubStyle {
    SubStyle {
        scale: f64::from(s.size) / 100.0,
        font: Some(s.font.trim().to_owned()).filter(|f| !f.is_empty()),
        color: s.color.clone(),
        back_alpha: s.bg_opacity,
        border_px: f64::from(s.outline),
        border_color: s.outline_color.clone(),
        pos: s.position,
        bold: s.bold,
    }
}

/// Called by `mod.rs` on every `PlayerEvent::Tracks` (tk-player sends it once per loaded file) with the track list
/// mpv reported. Applies the preferred subtitle / audio language (the preference wins over mpv's own pick), the
/// secondary subtitle, the subtitle style and the app fonts folder, and returns the `(subtitle id, audio id)` the
/// UI shows as selected (0 = off / none). `player` is `None` in tests (pure choice, nothing applied).
pub fn on_tracks(player: Option<&Arc<Player>>, list: &[TrackInfo], subs: &SubtitlePrefs, sources: &SourcePrefsVm) -> (i64, i64) {
    let choice = choose(list, subs, sources);
    let sid = choice.sub.unwrap_or_else(|| selected_id(list, TrackKind::Sub));
    let aid = choice.audio.unwrap_or_else(|| selected_id(list, TrackKind::Audio));
    let Some(p) = player else { return (sid, aid) };
    apply_fonts_dir(p);
    if let Err(e) = p.set_sub_style(&to_sub_style(&subs.style)) {
        tracing::debug!(error = %e, "subtitle style not applied");
    }
    if let Some(id) = choice.sub
        && selected_id(list, TrackKind::Sub) != id
        && let Err(e) = p.set_sub_track(Some(id))
    {
        tracing::warn!(error = %e, id, "preferred subtitle track not applied");
    }
    if let Some(id) = choice.audio
        && selected_id(list, TrackKind::Audio) != id
        && let Err(e) = p.set_audio_track(Some(id))
    {
        tracing::warn!(error = %e, id, "preferred audio track not applied");
    }
    if let Some(id) = choice.secondary
        && let Err(e) = p.set_secondary_sub_track(Some(id))
    {
        tracing::warn!(error = %e, id, "secondary subtitle not applied");
    }
    (sid, aid)
}

/// The subtitle menu picked `id` (0 = off): really switch mpv's `sid`. Integration: `mod.rs` `on_sub` calls this.
pub fn select_sub(player: Option<&Arc<Player>>, id: i64) {
    if let Some(p) = player
        && let Err(e) = p.set_sub_track(if id == 0 { None } else { Some(id) })
    {
        tracing::warn!(error = %e, id, "subtitle track not switched");
    }
}

/// The audio menu picked `id`: really switch mpv's `aid`. Integration: `mod.rs` `on_audio` calls this.
pub fn select_audio(player: Option<&Arc<Player>>, id: i64) {
    if let Some(p) = player
        && let Err(e) = p.set_audio_track(if id == 0 { None } else { Some(id) })
    {
        tracing::warn!(error = %e, id, "audio track not switched");
    }
}

// ---------------------------------------------------------------------------------------------
// Reading mpv
// ---------------------------------------------------------------------------------------------

/// One entry of mpv's `track-list` with what translation needs on top of [`TrackInfo`].
#[derive(Debug, Clone, PartialEq)]
struct TrackRow {
    info: TrackInfo,
    external: bool,
    /// `external-filename`: URL or path of an external track.
    filename: Option<String>,
    codec: Option<String>,
}

fn prop_flag(p: &Player, name: &str) -> bool {
    p.property_str(name).is_some_and(|v| v == "yes" || v == "true" || v == "1")
}

fn prop_id(p: &Player, name: &str) -> i64 {
    p.property_str(name).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0)
}

fn read_rows(p: &Player) -> Vec<TrackRow> {
    let count = prop_id(p, "track-list/count").clamp(0, 256);
    (0..count)
        .filter_map(|i| {
            let kind = match p.property_str(&format!("track-list/{i}/type"))?.as_str() {
                "video" => TrackKind::Video,
                "audio" => TrackKind::Audio,
                "sub" => TrackKind::Sub,
                _ => return None,
            };
            let id = p.property_str(&format!("track-list/{i}/id"))?.trim().parse::<i64>().ok()?;
            let text = |k: &str| p.property_str(&format!("track-list/{i}/{k}")).filter(|v| !v.is_empty());
            Some(TrackRow {
                info: TrackInfo { id, kind, lang: text("lang"), title: text("title"), selected: prop_flag(p, &format!("track-list/{i}/selected")) },
                external: prop_flag(p, &format!("track-list/{i}/external")),
                filename: text("external-filename"),
                codec: text("codec"),
            })
        })
        .collect()
}

/// mpv's current track list (same shape as `PlayerEvent::Tracks`). Integration: `mod.rs` re-reads it on
/// [`TRACKS_REV`].
pub fn read_tracks(p: &Player) -> Vec<TrackInfo> {
    read_rows(p).into_iter().map(|r| r.info).collect()
}

/// What mpv is doing right now (polled by the sheet).
#[derive(Debug, Clone, Default, PartialEq)]
struct Observed {
    tracks: Vec<TrackRow>,
    sid: i64,
    aid: i64,
    secondary: i64,
    delay_ms: i64,
    /// `path` of the loaded file ("" before any file).
    path: String,
}

fn observe(p: &Player) -> Observed {
    let delay = p.property_f64("sub-delay").unwrap_or(0.0);
    Observed {
        tracks: read_rows(p),
        sid: prop_id(p, "sid"),
        aid: prop_id(p, "aid"),
        secondary: prop_id(p, "secondary-sid"),
        delay_ms: if delay.is_finite() { (delay * 1000.0).round() as i64 } else { 0 },
        path: p.property_str("path").unwrap_or_default(),
    }
}

/// Bitmap subtitle codecs (no text to translate).
fn is_image_codec(codec: Option<&str>) -> bool {
    codec.is_some_and(|c| {
        let c = c.to_ascii_lowercase();
        c.contains("pgs") || c.contains("dvd_sub") || c.contains("dvb_sub") || c.contains("vobsub") || c.contains("xsub")
    })
}

/// URL or path `fetch_subtitle` can read (not mpv-internal schemes like `memory://`, `edl://`).
fn is_fetchable(f: &str) -> bool {
    f.starts_with("http://") || f.starts_with("https://") || f.starts_with("file://") || Path::new(f).is_absolute()
}

/// A track this module wrote (pre-translation output): never translated again.
fn is_ours(f: &str, subs_dir: &Path) -> bool {
    Path::new(f).starts_with(subs_dir)
}

fn lang_of(rows: &[TrackRow], id: i64) -> String {
    if id == 0 {
        return "off".to_owned();
    }
    rows.iter()
        .find(|r| r.info.id == id && r.info.kind != TrackKind::Video)
        .map(|r| track_lang(&r.info).map(str::to_owned).or_else(|| r.info.lang.clone()).unwrap_or_else(|| "und".to_owned()))
        .unwrap_or_else(|| "und".to_owned())
}

fn audio_lang_of(rows: &[TrackRow], id: i64) -> String {
    if id == 0 {
        return "off".to_owned();
    }
    rows.iter()
        .find(|r| r.info.id == id && r.info.kind == TrackKind::Audio)
        .map(|r| track_lang(&r.info).map(str::to_owned).or_else(|| r.info.lang.clone()).unwrap_or_else(|| "und".to_owned()))
        .unwrap_or_else(|| "und".to_owned())
}

fn sub_lang_of(rows: &[TrackRow], id: i64) -> String {
    if id == 0 {
        return "off".to_owned();
    }
    let subs: Vec<TrackRow> = rows.iter().filter(|r| r.info.kind == TrackKind::Sub).cloned().collect();
    lang_of(&subs, id)
}

/// The `data-*` the verifier reads from `.tk-pc` (camelCase dataset keys).
fn dataset(o: &Observed, style: &SubtitleStyleVm, translated: bool, last_capture: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "subLang": sub_lang_of(&o.tracks, o.sid),
        "audioLang": audio_lang_of(&o.tracks, o.aid),
        "secondaryLang": sub_lang_of(&o.tracks, o.secondary),
        "subDelayMs": o.delay_ms.to_string(),
        "subScale": format!("{:.2}", f64::from(style.size) / 100.0),
        "translated": translated.to_string(),
        "lastCapture": last_capture.unwrap_or(""),
    })
}

/// Installs (once) a mirror that keeps `attrs` on `.tk-pc` even when the controls re-mount, then sets `attrs`.
fn mirror_js(attrs: &serde_json::Value) -> String {
    format!(
        "(function(){{if(!window.__tkSubs){{var st={{attrs:{{}}}};st.apply=function(){{var el=document.querySelector('.tk-pc');\
         if(!el)return;for(var k in st.attrs){{var v=st.attrs[k];if(el.dataset[k]!==v)el.dataset[k]=v;}}}};\
         new MutationObserver(function(){{st.apply();}}).observe(document.body,{{childList:true,subtree:true}});window.__tkSubs=st;}}\
         window.__tkSubs.attrs={attrs};window.__tkSubs.apply();}})();"
    )
}

// ---------------------------------------------------------------------------------------------
// Screenshot
// ---------------------------------------------------------------------------------------------

/// The screenshot button (`PlayerControls::on_capture`) and the screenshot shortcut: `Backend::screenshot_path`
/// -> `Player::screenshot_to_file(path, with subtitles)` -> flash with the file name; errors flashed as text.
pub fn capture(ctx: &Flow, title: &str, episode: u32) {
    let Some(p) = ctx.player.peek().clone() else {
        ctx.flash(t("capture.no_player"));
        return;
    };
    let (ctx, title) = (ctx.clone(), title.to_owned());
    spawn(async move {
        let path = match ctx.backend.screenshot_path(&title, episode).await {
            Ok(path) => path,
            Err(e) => {
                ctx.flash(tf("capture.failed", &[("detail", &e.detail)]));
                return;
            }
        };
        match p.screenshot_to_file(&path, true) {
            Ok(()) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                *LAST_CAPTURE.write() = Some(path.to_string_lossy().into_owned());
                ctx.flash(tf("capture.saved", &[("name", &name)]));
            }
            Err(e) => ctx.flash(tf("capture.failed", &[("detail", &e.to_string())])),
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Translation jobs
// ---------------------------------------------------------------------------------------------

/// Pre-translation state (`data-translate-state`).
#[derive(Debug, Clone, Default, PartialEq)]
enum Pre {
    #[default]
    Idle,
    Running {
        done: usize,
        total: usize,
    },
    Done {
        label: String,
    },
    /// No external text track to translate (embedded or bitmap subtitles).
    Unsupported,
    Failed(UiError),
}

impl Pre {
    fn id(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running { .. } => "running",
            Self::Done { .. } => "done",
            Self::Unsupported => "unsupported",
            Self::Failed(_) => "error",
        }
    }
}

/// The external text track to pre-translate: the selected one, else the first, skipping tracks already in the
/// target language and our own output. `Err(true)` = only target-language text tracks (nothing to do),
/// `Err(false)` = no text track at all.
fn pretranslate_source(o: &Observed, target: &str, subs_dir: &Path) -> Result<TrackRow, bool> {
    let text_tracks: Vec<&TrackRow> = o
        .tracks
        .iter()
        .filter(|r| {
            r.info.kind == TrackKind::Sub
                && r.external
                && !is_image_codec(r.codec.as_deref())
                && r.filename.as_deref().is_some_and(|f| is_fetchable(f) && !is_ours(f, subs_dir))
        })
        .collect();
    if text_tracks.is_empty() {
        return Err(false);
    }
    let foreign: Vec<&&TrackRow> = text_tracks.iter().filter(|r| track_lang(&r.info) != lang_code(target)).collect();
    foreign
        .iter()
        .find(|r| r.info.id == o.sid)
        .or_else(|| foreign.first())
        .map(|r| (***r).clone())
        .ok_or(true)
}

/// The subtitle text of an external track (stream headers from mpv for remote files).
async fn load_text(backend: &BackendRef, p: &Player, row: &TrackRow) -> Result<String, UiError> {
    let f = row.filename.clone().ok_or_else(|| UiError::new(ErrorKind::Other, "the track has no file"))?;
    let mut headers: Vec<(String, String)> = Vec::new();
    if f.starts_with("http") {
        if let Some(r) = p.property_str("referrer").filter(|s| !s.is_empty()) {
            headers.push(("Referer".to_owned(), r));
        }
        if let Some(ua) = p.property_str("user-agent").filter(|s| !s.is_empty()) {
            headers.push(("User-Agent".to_owned(), ua));
        }
    }
    backend.fetch_subtitle(&f, &headers).await
}

fn io_err(e: std::io::Error) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

/// File-name safe stem: `<media>-e<ep>-<lang>`.
fn file_stem(media: &str, episode: u32, lang: &str) -> String {
    let safe: String = media.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{safe}-e{episode}-{lang}")
}

struct PreJob {
    backend: BackendRef,
    player: Arc<Player>,
    row: TrackRow,
    target: String,
    cache_key: String,
    subs_dir: PathBuf,
    stem: String,
    pre: Signal<Pre>,
    auto: bool,
}

/// Whole-track translation: fetch + parse + `translate_cues` in chunks (progress) + SRT in `<data>/subs/` +
/// `add_subtitle(select)`. An automatic run near the start pauses playback until the track is ready.
fn spawn_pretranslate(job: PreJob) {
    spawn(async move {
        let PreJob { backend, player, row, target, cache_key, subs_dir, stem, mut pre, auto } = job;
        pre.set(Pre::Running { done: 0, total: 0 });
        let pause = auto && player.property_f64("time-pos").unwrap_or(0.0) < AUTO_PAUSE_WITHIN_SECS && !prop_flag(&player, "pause");
        if pause {
            let _ = player.pause();
        }
        let result: Result<String, UiError> = async {
            let text = load_text(&backend, &player, &row).await?;
            let parsed = cues::parse(&text);
            if parsed.is_empty() {
                return Err(UiError::new(ErrorKind::Other, "the subtitle track has no text cues"));
            }
            let total = parsed.len();
            pre.set(Pre::Running { done: 0, total });
            let source = track_lang(&row.info).map(str::to_owned);
            let mut out: Vec<CueVm> = Vec::with_capacity(total);
            for chunk in parsed.chunks(PRE_CHUNK) {
                let req = TranslateRequestVm {
                    cues: chunk.iter().map(|c| CueVm { text: cues::plain(&c.text), ..c.clone() }).collect(),
                    source_lang: source.clone(),
                    target_lang: target.clone(),
                    cache_key: cache_key.clone(),
                };
                out.extend(backend.translate_cues(&req).await?);
                pre.set(Pre::Running { done: out.len(), total });
            }
            tokio::fs::create_dir_all(&subs_dir).await.map_err(io_err)?;
            let path = subs_dir.join(format!("{stem}.srt"));
            tokio::fs::write(&path, cues::to_srt(&out)).await.map_err(io_err)?;
            let label = tf("subs.translate.track_label", &[("lang", &target.to_uppercase())]);
            let src = SubtitleSource { url: path.to_string_lossy().into_owned(), lang: target.clone(), label: label.clone() };
            player.add_subtitle(&src, true).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
            Ok(label)
        }
        .await;
        if pause {
            let _ = player.play();
        }
        match result {
            Ok(label) => {
                pre.set(Pre::Done { label });
                bump_tracks();
            }
            Err(e) => pre.set(Pre::Failed(e)),
        }
    });
}

/// Where the live overlay draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LiveMode {
    #[default]
    Off,
    /// External text track: native subtitles hidden, the translated cue takes their place.
    Replace,
    /// Embedded text track (`sub-text`): the translation shows above the native line.
    Above,
    /// Bitmap track: nothing to translate.
    Unsupported,
}

impl LiveMode {
    fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Replace => "replace",
            Self::Above => "above",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Live translation memory of the current track.
#[derive(Default)]
struct LiveCache {
    /// `<path>#<sid>`; a change resets everything.
    track: String,
    cues: Option<Vec<CueVm>>,
    loading: bool,
    load_failed: bool,
    /// `cues::key(original)` -> translation.
    map: HashMap<String, String>,
    pending: HashSet<String>,
    inflight: bool,
    /// A failed request stops new ones for this track until "Retry".
    failed: bool,
}

/// Translate `items` (original texts) and store them under their keys.
fn spawn_live_batch(backend: BackendRef, cache: Rc<RefCell<LiveCache>>, req: TranslateRequestVm, track: String, mut err: Signal<Option<UiError>>) {
    spawn(async move {
        let keys: Vec<String> = req.cues.iter().map(|c| cues::key(&c.text)).collect();
        let res = backend.translate_cues(&req).await;
        let mut c = cache.borrow_mut();
        if c.track != track {
            return;
        }
        c.inflight = false;
        for k in &keys {
            c.pending.remove(k);
        }
        match res {
            Ok(list) => {
                for (k, cue) in keys.into_iter().zip(list) {
                    c.map.insert(k, cues::plain(&cue.text));
                }
            }
            Err(e) => {
                c.failed = true;
                drop(c);
                err.set(Some(e));
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------------------------

/// Installed families (+ the app fonts folder); `rescan` ignores the cache.
async fn font_families(extra: PathBuf, rescan: bool) -> Vec<String> {
    if !rescan && let Some(v) = FONT_CACHE.lock().ok().and_then(|g| g.clone()) {
        return v;
    }
    let list = tokio::task::spawn_blocking(move || fonts::installed_families(&extra)).await.unwrap_or_default();
    if let Ok(mut g) = FONT_CACHE.lock() {
        *g = Some(list.clone());
    }
    list
}

/// Font families in the app fonts folder only (Settings shows how many it found).
pub(crate) async fn app_font_families(dir: PathBuf) -> Vec<String> {
    tokio::task::spawn_blocking(move || fonts::families_in(&dir)).await.unwrap_or_default()
}

/// `(families, loading)`; the second hook value restarts the scan.
pub(crate) fn use_font_families(extra: PathBuf) -> (Resource<Vec<String>>, Signal<bool>) {
    let rescan = use_signal(|| false);
    let res = use_resource(move || {
        let extra = extra.clone();
        let force = rescan();
        async move { font_families(extra, force).await }
    });
    (res, rescan)
}

// ---------------------------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------------------------

/// Persist new subtitle preferences.
pub(crate) fn commit_subs(ui: Ui, backend: &BackendRef, next: SubtitlePrefs) {
    let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetSubs(next.sanitized()));
    ui.commit_settings(backend, s);
}

/// Inline style of a subtitle line drawn by the web view (preview, live overlay). Colours are user data.
pub(crate) fn line_style(s: &SubtitleStyleVm, base_px: f64) -> String {
    let font = if s.font.trim().is_empty() { "var(--tk-font-text)".to_owned() } else { format!("\"{}\", var(--tk-font-text)", s.font.replace('"', "")) };
    let size = base_px * f64::from(s.size) / 100.0;
    let o = f64::from(s.outline);
    let shadow = if s.outline == 0 {
        "none".to_owned()
    } else {
        let c = &s.outline_color;
        format!("{o}px 0 0 {c}, -{o}px 0 0 {c}, 0 {o}px 0 {c}, 0 -{o}px 0 {c}, {o}px {o}px 0 {c}, -{o}px -{o}px 0 {c}, {o}px -{o}px 0 {c}, -{o}px {o}px 0 {c}")
    };
    format!(
        "font-family:{font};font-size:{size:.1}px;color:{};font-weight:{};text-shadow:{shadow};\
         background-color:color-mix(in srgb, var(--md-sys-color-scrim) {}%, transparent);",
        s.color,
        if s.bold { 700 } else { 400 },
        s.bg_opacity
    )
}

/// Live preview of a style: a sample line at the chosen height of a 16:9 frame.
#[component]
pub(crate) fn StylePreview(style: SubtitleStyleVm) -> Element {
    let bottom = f64::from(100 - style.position.min(100)) * 0.8 + 6.0;
    let line = line_style(&style, 20.0);
    let scale = format!("{:.2}", f64::from(style.size) / 100.0);
    rsx! {
        div { class: "tk-subs-preview", "data-sub-scale": "{scale}", "aria-label": t("subs.style.preview"),
            span { class: "tk-subs-preview__line", style: "{line}bottom:{bottom:.1}%;", {t("subs.style.sample")} }
        }
    }
}

/// Style controls (size, font, colour, outline, background, position, bold, reset) with a live preview. Shared by
/// the player sheet and Settings > Subtitles.
#[component]
pub(crate) fn StyleEditor(style: SubtitleStyleVm, fonts: Vec<String>, #[props(default)] fonts_loading: bool, on_change: EventHandler<SubtitleStyleVm>) -> Element {
    let make = |apply: fn(&mut SubtitleStyleVm, &str)| {
        let base = style.clone();
        move |e: FormEvent| {
            let mut s = base.clone();
            apply(&mut s, &e.value());
            on_change.call(s.sanitized());
        }
    };
    let bold_base = style.clone();
    let (font_gen, font_record) = use_uncontrolled_input(&style.font);
    let font_base = style.clone();
    rsx! {
        div { class: "tk-subs-style", "data-sub-scale": format!("{:.2}", f64::from(style.size) / 100.0),
            StylePreview { style: style.clone() }
            label { class: "tk-subs-ctl",
                span { class: "tk-subs-ctl__label", {t("subs.style.size")} }
                input {
                    r#type: "range", min: "50", max: "200", step: "5", value: "{style.size}",
                    "aria-label": t("subs.style.size"),
                    oninput: make(|s, v| if let Ok(n) = v.parse::<u8>() { s.size = n }),
                }
                span { class: "tk-subs-ctl__value", "{style.size} %" }
            }
            label { class: "tk-subs-ctl",
                span { class: "tk-subs-ctl__label", {t("subs.style.font")} }
                {std::iter::once(font_gen).map(|generation| {
                    let record = font_record.clone();
                    let base = font_base.clone();
                    rsx! {
                        input {
                            key: "{generation}",
                            class: "tk-subs-input",
                            r#type: "text",
                            list: "tk-subs-fonts",
                            initial_value: "{style.font}",
                            placeholder: t("subs.style.font_default"),
                            autocomplete: "off",
                            spellcheck: "false",
                            oninput: move |e: FormEvent| {
                                let v = e.value();
                                record(&v);
                                on_change.call(SubtitleStyleVm { font: v, ..base.clone() }.sanitized());
                            },
                        }
                    }
                })}
                if fonts_loading {
                    LoadingIndicator { small: true }
                }
                datalist { id: "tk-subs-fonts",
                    for f in fonts.iter() {
                        option { key: "{f}", value: "{f}" }
                    }
                }
            }
            div { class: "tk-subs-ctl tk-subs-ctl--row",
                label { class: "tk-subs-swatch",
                    span { class: "tk-subs-ctl__label", {t("subs.style.color")} }
                    input {
                        r#type: "color", value: "{style.color.to_ascii_lowercase()}",
                        "aria-label": t("subs.style.color"),
                        oninput: make(|s, v| s.color = v.to_ascii_uppercase()),
                    }
                }
                label { class: "tk-subs-swatch",
                    span { class: "tk-subs-ctl__label", {t("subs.style.outline_color")} }
                    input {
                        r#type: "color", value: "{style.outline_color.to_ascii_lowercase()}",
                        "aria-label": t("subs.style.outline_color"),
                        oninput: make(|s, v| s.outline_color = v.to_ascii_uppercase()),
                    }
                }
            }
            label { class: "tk-subs-ctl",
                span { class: "tk-subs-ctl__label", {t("subs.style.outline")} }
                input {
                    r#type: "range", min: "0", max: "5", step: "1", value: "{style.outline}",
                    "aria-label": t("subs.style.outline"),
                    oninput: make(|s, v| if let Ok(n) = v.parse::<u8>() { s.outline = n }),
                }
                span { class: "tk-subs-ctl__value", "{style.outline} px" }
            }
            label { class: "tk-subs-ctl",
                span { class: "tk-subs-ctl__label", {t("subs.style.background")} }
                input {
                    r#type: "range", min: "0", max: "100", step: "5", value: "{style.bg_opacity}",
                    "aria-label": t("subs.style.background"),
                    oninput: make(|s, v| if let Ok(n) = v.parse::<u8>() { s.bg_opacity = n }),
                }
                span { class: "tk-subs-ctl__value", "{style.bg_opacity} %" }
            }
            label { class: "tk-subs-ctl",
                span { class: "tk-subs-ctl__label", {t("subs.style.position")} }
                input {
                    r#type: "range", min: "0", max: "100", step: "1", value: "{style.position}",
                    "aria-label": t("subs.style.position"),
                    oninput: make(|s, v| if let Ok(n) = v.parse::<u8>() { s.position = n }),
                }
                span { class: "tk-subs-ctl__value", "{style.position}" }
            }
            div { class: "tk-subs-ctl tk-subs-ctl--row",
                span { class: "tk-subs-ctl__label", {t("subs.style.bold")} }
                Switch {
                    checked: style.bold,
                    label: t("subs.style.bold"),
                    on_toggle: move |v: bool| on_change.call(SubtitleStyleVm { bold: v, ..bold_base.clone() }),
                }
            }
            div { class: "tk-subs-actions",
                Button {
                    label: t("subs.style.reset"),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    icon: "refresh".to_string(),
                    onclick: move |_| on_change.call(SubtitleStyleVm::default()),
                }
            }
        }
    }
}

/// Sheet opened from the subtitles menu ("Subtitle style..."): tracks, delay (-/+ 100 ms, reset), secondary
/// subtitle, "Load file..." (SRT / ASS / SSA / VTT via `Player::add_subtitle`), style with live preview and
/// translation (live toggle, translate the track now). Always mounted by `mod.rs`: while closed it still runs the
/// mpv poll, the translation jobs and draws the live-translation overlay.
#[component]
pub fn SubtitleSheet(
    open: bool,
    player: PlayerHandle,
    tracks: Vec<TrackInfo>,
    /// Selected subtitle track id (0 = off).
    sub_track: i64,
    #[props(default)] on_close: EventHandler<()>,
) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let route = use_route::<Route>();
    let (media_id, episode) = match route {
        Route::PlayerPage { id, episode } => (id, episode),
        _ => (String::new(), 0),
    };

    // The latest handle for the long-running futures (props change, the futures stay).
    let cell = use_hook(|| Rc::new(RefCell::new(PlayerHandle::default())));
    if *cell.borrow() != player {
        *cell.borrow_mut() = player.clone();
    }
    let data_dir = use_hook(|| backend.data_dir_display());
    let subs_dir = use_hook(|| if data_dir.is_empty() { std::env::temp_dir().join("tatakai-next-subs") } else { Path::new(&data_dir).join("subs") });
    let fonts_dir = use_hook(|| {
        let dir = app_fonts_dir(&data_dir);
        remember_fonts_dir(&dir);
        dir
    });
    let (fonts_res, _rescan) = use_font_families(fonts_dir.clone());

    let mut obs = use_signal(Observed::default);
    let pre = use_signal(|| Pre::Idle);
    let mut live_line = use_signal(|| None::<String>);
    let mut live_mode = use_signal(LiveMode::default);
    let live_err = use_signal(|| None::<UiError>);
    let mut file_err = use_signal(|| None::<UiError>);
    let live_cache = use_hook(|| Rc::new(RefCell::new(LiveCache::default())));
    let hidden = use_hook(|| Rc::new(Cell::new(false)));
    let pre_for = use_hook(|| Rc::new(RefCell::new(String::new())));

    // Poll mpv, auto pre-translate, mirror the state onto `.tk-pc`.
    use_future({
        let (cell, backend, subs_dir, pre_for, media_id) = (cell.clone(), backend.clone(), subs_dir.clone(), pre_for.clone(), media_id.clone());
        move || {
            let (cell, backend, subs_dir, pre_for, media_id) = (cell.clone(), backend.clone(), subs_dir.clone(), pre_for.clone(), media_id.clone());
            async move {
                let mut last = String::new();
                let mut fonts_for: Option<Arc<Player>> = None;
                loop {
                    tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
                    let p = cell.borrow().0.clone();
                    let prefs = ui.settings.peek().subs.clone();
                    // The app fonts folder reaches libass as soon as a player exists (the instance is reused by
                    // the next files, which then load with it), not only from `on_tracks`.
                    if let Some(p) = &p
                        && !fonts_for.as_ref().is_some_and(|f| Arc::ptr_eq(f, p))
                    {
                        fonts_for = Some(p.clone());
                        apply_fonts_dir(p);
                    }
                    if let Some(p) = &p {
                        let o = observe(p);
                        if *obs.peek() != o {
                            obs.set(o);
                        }
                    }
                    let o = obs.peek().clone();
                    let tr = &prefs.translation;
                    if let Some(p) = &p
                        && tr.pretranslate
                        && tr.provider != TranslationProviderVm::Off
                        && !o.path.is_empty()
                        && !o.tracks.is_empty()
                        && *pre_for.borrow() != o.path
                        && !matches!(*pre.peek(), Pre::Running { .. })
                    {
                        *pre_for.borrow_mut() = o.path.clone();
                        match pretranslate_source(&o, &tr.target_lang, &subs_dir) {
                            Ok(row) => spawn_pretranslate(PreJob {
                                backend: backend.clone(),
                                player: p.clone(),
                                cache_key: format!("{media_id}:{episode}:{}", track_lang(&row.info).unwrap_or("und")),
                                row,
                                target: tr.target_lang.clone(),
                                subs_dir: subs_dir.clone(),
                                stem: file_stem(&media_id, episode, &tr.target_lang),
                                pre,
                                auto: true,
                            }),
                            Err(false) => {
                                let mut pre = pre;
                                pre.set(Pre::Unsupported);
                            }
                            Err(true) => {}
                        }
                    }
                    let translated = matches!(*pre.peek(), Pre::Done { .. }) || matches!(*live_mode.peek(), LiveMode::Replace | LiveMode::Above);
                    let capture = LAST_CAPTURE.peek().clone();
                    let attrs = dataset(&o, &prefs.style, translated, capture.as_deref());
                    let js = mirror_js(&attrs);
                    if js != last {
                        let _ = document::eval(&js);
                        last = js;
                    }
                }
            }
        }
    });

    // Live translation of the visible cue.
    use_future({
        let (cell, backend, subs_dir, cache, hidden, media_id) =
            (cell.clone(), backend.clone(), subs_dir.clone(), live_cache.clone(), hidden.clone(), media_id.clone());
        move || {
            let (cell, backend, subs_dir, cache, hidden, media_id) =
                (cell.clone(), backend.clone(), subs_dir.clone(), cache.clone(), hidden.clone(), media_id.clone());
            async move {
                let restore = |p: &Player, hidden: &Cell<bool>| {
                    if hidden.replace(false) {
                        let _ = p.set_option_str("sub-visibility", "yes");
                    }
                };
                loop {
                    tokio::time::sleep(Duration::from_millis(LIVE_TICK_MS)).await;
                    let prefs = ui.settings.peek().subs.translation.clone();
                    let p = cell.borrow().0.clone();
                    let Some(p) = p else { continue };
                    let on = prefs.live && prefs.provider != TranslationProviderVm::Off && !matches!(*pre.peek(), Pre::Done { .. });
                    let o = obs.peek().clone();
                    let row = o.tracks.iter().find(|r| r.info.kind == TrackKind::Sub && r.info.id == o.sid).cloned();
                    let Some(row) = row.filter(|_| on) else {
                        restore(&p, &hidden);
                        if live_line.peek().is_some() {
                            live_line.set(None);
                        }
                        if *live_mode.peek() != LiveMode::Off {
                            live_mode.set(LiveMode::Off);
                        }
                        continue;
                    };
                    if is_image_codec(row.codec.as_deref()) {
                        restore(&p, &hidden);
                        if *live_mode.peek() != LiveMode::Unsupported {
                            live_mode.set(LiveMode::Unsupported);
                            live_line.set(None);
                        }
                        continue;
                    }
                    let track = format!("{}#{}", o.path, row.info.id);
                    {
                        let mut c = cache.borrow_mut();
                        if c.track != track {
                            *c = LiveCache { track: track.clone(), ..LiveCache::default() };
                        }
                    }
                    let source = track_lang(&row.info).map(str::to_owned);
                    let cache_key = format!("{media_id}:{episode}:{}", source.as_deref().unwrap_or("und"));
                    let external = row.external && row.filename.as_deref().is_some_and(|f| is_fetchable(f) && !is_ours(f, &subs_dir));
                    let failed = cache.borrow().failed;

                    if external && !cache.borrow().load_failed {
                        let need_load = {
                            let c = cache.borrow();
                            c.cues.is_none() && !c.loading
                        };
                        if need_load {
                            cache.borrow_mut().loading = true;
                            let (backend, cache, p, row, track) = (backend.clone(), cache.clone(), p.clone(), row.clone(), track.clone());
                            spawn(async move {
                                let loaded = load_text(&backend, &p, &row).await.map(|text| cues::parse(&text));
                                let mut c = cache.borrow_mut();
                                if c.track != track {
                                    return;
                                }
                                c.loading = false;
                                match loaded {
                                    Ok(list) if !list.is_empty() => c.cues = Some(list),
                                    _ => c.load_failed = true,
                                }
                            });
                        }
                        let ready = cache.borrow().cues.is_some();
                        if !ready {
                            continue;
                        }
                        if !hidden.replace(true) {
                            let _ = p.set_option_str("sub-visibility", "no");
                        }
                        if *live_mode.peek() != LiveMode::Replace {
                            live_mode.set(LiveMode::Replace);
                        }
                        let pos = p.property_f64("time-pos").unwrap_or(0.0);
                        let delay = p.property_f64("sub-delay").unwrap_or(0.0);
                        let ms = ((pos - delay).max(0.0) * 1000.0) as u64;
                        let (line, batch) = {
                            let c = cache.borrow();
                            let list = c.cues.as_deref().unwrap_or(&[]);
                            let line = cues::cue_at(list, ms).map(|i| {
                                c.map.get(&cues::key(&list[i].text)).cloned().unwrap_or_default()
                            });
                            let batch: Vec<CueVm> = if c.inflight || failed {
                                Vec::new()
                            } else {
                                list[cues::cue_from(list, ms)..]
                                    .iter()
                                    .filter(|cue| {
                                        let k = cues::key(&cue.text);
                                        !c.map.contains_key(&k) && !c.pending.contains(&k)
                                    })
                                    .take(LIVE_AHEAD)
                                    .map(|cue| CueVm { text: cues::plain(&cue.text), ..cue.clone() })
                                    .collect()
                            };
                            (line.filter(|l| !l.is_empty()), batch)
                        };
                        if *live_line.peek() != line {
                            live_line.set(line);
                        }
                        if !batch.is_empty() {
                            {
                                let mut c = cache.borrow_mut();
                                c.inflight = true;
                                for cue in &batch {
                                    c.pending.insert(cues::key(&cue.text));
                                }
                            }
                            let req = TranslateRequestVm { cues: batch, source_lang: source.clone(), target_lang: prefs.target_lang.clone(), cache_key };
                            spawn_live_batch(backend.clone(), cache.clone(), req, track.clone(), live_err);
                        }
                    } else {
                        // Embedded text track (or an external one that could not be read): mpv's current line.
                        restore(&p, &hidden);
                        if *live_mode.peek() != LiveMode::Above {
                            live_mode.set(LiveMode::Above);
                        }
                        let text = p.property_str("sub-text").unwrap_or_default();
                        let k = cues::key(&text);
                        if k.is_empty() {
                            if live_line.peek().is_some() {
                                live_line.set(None);
                            }
                            continue;
                        }
                        let (hit, ask) = {
                            let c = cache.borrow();
                            (c.map.get(&k).cloned(), !failed && !c.inflight && !c.pending.contains(&k) && !c.map.contains_key(&k))
                        };
                        if *live_line.peek() != hit {
                            live_line.set(hit);
                        }
                        if ask {
                            {
                                let mut c = cache.borrow_mut();
                                c.inflight = true;
                                c.pending.insert(k.clone());
                            }
                            let cue = CueVm { index: 0, start_ms: 0, end_ms: 0, text: cues::plain(&text) };
                            let req = TranslateRequestVm { cues: vec![cue], source_lang: source.clone(), target_lang: prefs.target_lang.clone(), cache_key };
                            spawn_live_batch(backend.clone(), cache.clone(), req, track.clone(), live_err);
                        }
                    }
                }
            }
        }
    });

    // Native subtitles come back when the screen goes (the player instance is reused by the next screen).
    use_drop({
        let (cell, hidden) = (cell.clone(), hidden.clone());
        move || {
            if hidden.replace(false)
                && let Some(p) = cell.borrow().0.clone()
            {
                let _ = p.set_option_str("sub-visibility", "yes");
            }
        }
    });

    let prefs = ui.settings.read().subs.clone();
    let o = obs();
    let pre_now = pre();
    let mode = live_mode();
    let line = live_line();
    let fonts_list = fonts_res.read().clone().unwrap_or_default();
    let fonts_loading = fonts_res.read().is_none();
    // Prefer mpv's live list; before the first poll, the props.
    let sub_rows: Vec<TrackInfo> = if o.tracks.is_empty() {
        tracks.iter().filter(|tr| tr.kind == TrackKind::Sub).cloned().collect()
    } else {
        o.tracks.iter().filter(|r| r.info.kind == TrackKind::Sub).map(|r| r.info.clone()).collect()
    };
    let current_sid = if o.tracks.is_empty() { sub_track } else { o.sid };
    let has_player = player.0.is_some();

    let overlay = match (&line, mode) {
        (Some(text), LiveMode::Replace | LiveMode::Above) => {
            let style = line_style(&prefs.style, 30.0);
            let pos = if mode == LiveMode::Replace { "bottom" } else { "top" };
            let bottom = if mode == LiveMode::Replace { format!("bottom:{:.1}%;", f64::from(100 - prefs.style.position.min(100)) * 0.8 + 6.0) } else { String::new() };
            rsx! {
                div { class: "tk-subs-live", "data-pos": "{pos}", "aria-live": "polite",
                    span { class: "tk-subs-live__line", style: "{style}{bottom}", "{text}" }
                }
            }
        }
        _ => rsx! {},
    };
    let chip = match (&pre_now, mode) {
        (Pre::Running { done, total }, _) => {
            let pct = if *total == 0 { 0 } else { done * 100 / total };
            Some(tf("subs.translate.progress", &[("pct", &pct.to_string())]))
        }
        (_, LiveMode::Unsupported) => Some(t("subs.translate.internal_only")),
        _ => None,
    };

    // Sheet actions.
    let set_delay = {
        let cell = cell.clone();
        move |ms: i64| {
            if let Some(p) = cell.borrow().0.clone() {
                let _ = p.set_sub_delay(ms as f64 / 1000.0);
            }
            // A local copy keeps this closure `Fn` (it is cloned into several buttons).
            let mut obs = obs;
            obs.write().delay_ms = ms;
        }
    };
    let pick_sub = {
        let cell = cell.clone();
        move |id: i64| {
            let p = cell.borrow().0.clone();
            select_sub(p.as_ref(), id);
            let mut obs = obs;
            obs.write().sid = id;
            bump_tracks();
        }
    };
    let pick_secondary = {
        let (cell, backend) = (cell.clone(), backend.clone());
        move |id: i64, lang: Option<String>| {
            if let Some(p) = cell.borrow().0.clone() {
                let _ = p.set_secondary_sub_track(if id == 0 { None } else { Some(id) });
            }
            let mut obs = obs;
            obs.write().secondary = id;
            let mut next = ui.settings.peek().subs.clone();
            next.secondary_lang = lang;
            commit_subs(ui, &backend, next);
        }
    };
    let change_style = {
        let (cell, backend) = (cell.clone(), backend.clone());
        move |s: SubtitleStyleVm| {
            if let Some(p) = cell.borrow().0.clone() {
                let _ = p.set_sub_style(&to_sub_style(&s));
            }
            let mut next = ui.settings.peek().subs.clone();
            next.style = s;
            commit_subs(ui, &backend, next);
        }
    };
    let load_file = {
        let cell = cell.clone();
        move |path: PathBuf, name: String| {
            let Some(p) = cell.borrow().0.clone() else { return };
            let mut file_err = file_err;
            // "Episode 1.es.srt" -> es
            let lang = Path::new(&name)
                .file_stem()
                .and_then(|s| Path::new(s).extension())
                .and_then(|x| x.to_str())
                .and_then(lang_code)
                .unwrap_or("")
                .to_owned();
            let src = SubtitleSource { url: path.to_string_lossy().into_owned(), lang, label: name.clone() };
            match p.add_subtitle(&src, true) {
                Ok(()) => {
                    file_err.set(None);
                    bump_tracks();
                    snack.show(tf("subs.file.loaded", &[("name", &name)]));
                }
                Err(e) => file_err.set(Some(UiError::new(ErrorKind::Other, e.to_string()))),
            }
        }
    };
    let translate_now = {
        let (cell, backend, subs_dir, media_id) = (cell.clone(), backend.clone(), subs_dir.clone(), media_id.clone());
        move |_: ()| {
            let Some(p) = cell.borrow().0.clone() else { return };
            let tr = ui.settings.peek().subs.translation.clone();
            let o = obs.peek().clone();
            match pretranslate_source(&o, &tr.target_lang, &subs_dir) {
                Ok(row) => spawn_pretranslate(PreJob {
                    backend: backend.clone(),
                    player: p,
                    cache_key: format!("{media_id}:{episode}:{}", track_lang(&row.info).unwrap_or("und")),
                    row,
                    target: tr.target_lang.clone(),
                    subs_dir: subs_dir.clone(),
                    stem: file_stem(&media_id, episode, &tr.target_lang),
                    pre,
                    auto: false,
                }),
                Err(true) => snack.show(t("subs.translate.already_target")),
                Err(false) => {
                    let mut pre = pre;
                    pre.set(Pre::Unsupported);
                }
            }
        }
    };
    let retry_live = {
        let cache = live_cache.clone();
        move |_: ()| {
            cache.borrow_mut().track.clear();
            let mut e = live_err;
            e.set(None);
        }
    };
    let toggle_live = {
        let backend = backend.clone();
        move |v: bool| {
            let mut next = ui.settings.peek().subs.clone();
            next.translation.live = v;
            commit_subs(ui, &backend, next);
        }
    };

    let delay = o.delay_ms;
    let provider_off = prefs.translation.provider == TranslationProviderVm::Off;
    let secondary_id = o.secondary;

    rsx! {
        {overlay}
        if let Some(text) = chip {
            div { class: "tk-subs-chip", role: "status", "data-translate-state": pre_now.id(), "data-live-mode": mode.id(),
                if matches!(pre_now, Pre::Running { .. }) {
                    LoadingIndicator { small: true }
                }
                span { "{text}" }
            }
        }
        div { class: "tk-subs-sheet", "data-open": "{open}", "data-sub-delay-ms": "{delay}",
            "data-translate-state": pre_now.id(), "data-live-mode": mode.id(),
            if open {
                Sheet { title: t("subs.sheet.title"), on_close: move |_| on_close.call(()),
                    if !has_player {
                        p { class: "tk-panel__hint", {t("subs.sheet.no_player")} }
                    }
                    // Tracks
                    section { class: "tk-subs-sec",
                        h3 { class: "tk-subs-sec__title", {t("subs.tracks")} }
                        if sub_rows.is_empty() {
                            p { class: "tk-panel__hint", {t("subs.tracks.none")} }
                        }
                        div { class: "tk-subs-tracks", role: "radiogroup", "aria-label": t("subs.tracks"),
                            button {
                                class: "tk-subs-track", r#type: "button", role: "radio",
                                "aria-checked": "{current_sid == 0}", "data-selected": "{current_sid == 0}",
                                onclick: {
                                    let pick = pick_sub.clone();
                                    move |_| pick(0)
                                },
                                {t("subs.off")}
                            }
                            for tr in sub_rows.iter() {
                                {
                                    let id = tr.id;
                                    let label = track_label(tr);
                                    let pick = pick_sub.clone();
                                    rsx! {
                                        button {
                                            key: "{id}",
                                            class: "tk-subs-track", r#type: "button", role: "radio",
                                            "aria-checked": "{current_sid == id}", "data-selected": "{current_sid == id}",
                                            "data-lang": track_lang(tr).unwrap_or("und"),
                                            onclick: move |_| pick(id),
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "tk-subs-actions",
                            Button {
                                label: t("subs.file.load"),
                                icon: "upload".to_string(),
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: !has_player,
                                onclick: move |_| {
                                    let _ = document::eval("document.getElementById('tk-subs-file')?.click()");
                                },
                            }
                            input {
                                id: "tk-subs-file",
                                r#type: "file",
                                accept: ".srt,.ass,.ssa,.vtt,.sub",
                                hidden: true,
                                onchange: {
                                    let load = load_file.clone();
                                    move |e: FormEvent| {
                                        if let Some(f) = e.files().first() {
                                            load(f.path(), f.name());
                                        }
                                    }
                                },
                            }
                        }
                        if let Some(e) = file_err() {
                            ErrorState { error: e, compact: true, retry: false, title: t("subs.file.failed") }
                        }
                    }
                    // Delay
                    section { class: "tk-subs-sec",
                        h3 { class: "tk-subs-sec__title", {t("subs.delay")} }
                        div { class: "tk-subs-delay", "data-sub-delay-ms": "{delay}",
                            Button {
                                label: t("subs.delay.earlier"),
                                variant: ButtonVariant::Outlined,
                                size: ButtonSize::Sm,
                                disabled: !has_player,
                                onclick: {
                                    let set = set_delay.clone();
                                    move |_| set(delay - DELAY_STEP_MS)
                                },
                            }
                            span { class: "tk-subs-delay__value", role: "status",
                                {tf("subs.delay.value", &[("ms", &format!("{delay:+}"))])}
                            }
                            Button {
                                label: t("subs.delay.later"),
                                variant: ButtonVariant::Outlined,
                                size: ButtonSize::Sm,
                                disabled: !has_player,
                                onclick: {
                                    let set = set_delay.clone();
                                    move |_| set(delay + DELAY_STEP_MS)
                                },
                            }
                            Button {
                                label: t("subs.delay.reset"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                disabled: !has_player || delay == 0,
                                onclick: {
                                    let set = set_delay.clone();
                                    move |_| set(0)
                                },
                            }
                        }
                    }
                    // Secondary
                    section { class: "tk-subs-sec",
                        h3 { class: "tk-subs-sec__title", {t("subs.secondary")} }
                        p { class: "tk-panel__hint", {t("subs.secondary.hint")} }
                        div { class: "tk-subs-tracks", role: "radiogroup", "aria-label": t("subs.secondary"),
                            "data-secondary-lang": sub_lang_of(&o.tracks, secondary_id),
                            button {
                                class: "tk-subs-track", r#type: "button", role: "radio",
                                "aria-checked": "{secondary_id == 0}", "data-selected": "{secondary_id == 0}",
                                onclick: {
                                    let pick = pick_secondary.clone();
                                    move |_| pick(0, None)
                                },
                                {t("subs.off")}
                            }
                            for tr in sub_rows.iter().filter(|tr| tr.id != current_sid) {
                                {
                                    let id = tr.id;
                                    let label = track_label(tr);
                                    let lang = track_lang(tr).map(str::to_owned);
                                    let pick = pick_secondary.clone();
                                    rsx! {
                                        button {
                                            key: "{id}",
                                            class: "tk-subs-track", r#type: "button", role: "radio",
                                            "aria-checked": "{secondary_id == id}", "data-selected": "{secondary_id == id}",
                                            onclick: move |_| pick(id, lang.clone()),
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Style
                    section { class: "tk-subs-sec",
                        h3 { class: "tk-subs-sec__title", {t("subs.style")} }
                        StyleEditor { style: prefs.style.clone(), fonts: fonts_list, fonts_loading, on_change: change_style }
                    }
                    // Translation
                    section { class: "tk-subs-sec", "data-translate-state": pre_now.id(),
                        h3 { class: "tk-subs-sec__title", {t("subs.translate")} }
                        if provider_off {
                            p { class: "tk-panel__hint", {t("subs.translate.off_hint")} }
                        } else {
                            div { class: "tk-subs-ctl tk-subs-ctl--row",
                                span { class: "tk-subs-ctl__label", {t("translate.live")} }
                                Switch { checked: prefs.translation.live, label: t("translate.live"), on_toggle: toggle_live }
                            }
                            match &pre_now {
                                Pre::Running { done, total } => rsx! {
                                    div { class: "tk-subs-progress", role: "status",
                                        LoadingIndicator { small: true }
                                        span { {tf("subs.translate.progress_n", &[("done", &done.to_string()), ("total", &total.to_string())])} }
                                    }
                                },
                                Pre::Done { label } => rsx! {
                                    p { class: "tk-panel__hint", {tf("subs.translate.done", &[("label", label)])} }
                                },
                                Pre::Unsupported => rsx! {
                                    p { class: "tk-panel__hint", {t("subs.translate.internal_only")} }
                                },
                                Pre::Failed(e) => rsx! {
                                    ErrorState { error: e.clone(), compact: true, title: t("subs.translate.failed"), on_retry: translate_now.clone() }
                                },
                                Pre::Idle => rsx! {},
                            }
                            if mode == LiveMode::Unsupported {
                                p { class: "tk-panel__hint", {t("subs.translate.internal_only")} }
                            }
                            if let Some(e) = live_err() {
                                ErrorState { error: e, compact: true, title: t("subs.translate.live_failed"), on_retry: retry_live.clone() }
                            }
                            div { class: "tk-subs-actions",
                                Button {
                                    label: t("subs.translate.now"),
                                    icon: "language".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    size: ButtonSize::Sm,
                                    disabled: !has_player || matches!(pre_now, Pre::Running { .. }),
                                    onclick: {
                                        let go = translate_now.clone();
                                        move |_| go(())
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(id: i64, kind: TrackKind, lang: &str, title: Option<&str>, selected: bool) -> TrackInfo {
        TrackInfo { id, kind, lang: Some(lang.to_owned()).filter(|l| !l.is_empty()), title: title.map(str::to_owned), selected }
    }

    fn prefs(sub: &str, dub: &str) -> SourcePrefsVm {
        SourcePrefsVm { sub_lang: sub.to_owned(), dub_lang: dub.to_owned(), ..SourcePrefsVm::default() }
    }

    #[test]
    fn language_tags_and_names_normalise() {
        assert_eq!(lang_code("es-419"), Some("es"));
        assert_eq!(lang_code("spa"), Some("es"));
        assert_eq!(lang_code("Español (Latino)"), Some("es"));
        assert_eq!(lang_code("pt-BR"), Some("pt"));
        assert_eq!(lang_code("jpn"), Some("ja"));
        assert_eq!(lang_code("und"), None);
        assert_eq!(track_lang(&tr(1, TrackKind::Sub, "", Some("Latin American Spanish"), false)), Some("es"));
    }

    /// The audit bug: with preference ES, a stream whose ES-419 / ES tracks mpv left off starts with one of them.
    #[test]
    fn preferred_language_wins_over_mpv_selection() {
        let list = vec![
            tr(1, TrackKind::Sub, "en", None, true),
            tr(2, TrackKind::Sub, "spa", Some("Signs & Songs"), false),
            tr(3, TrackKind::Sub, "es-419", Some("Full"), false),
            tr(1, TrackKind::Audio, "en", None, true),
            tr(2, TrackKind::Audio, "ja", None, false),
        ];
        let (sid, aid) = on_tracks(None, &list, &SubtitlePrefs::default(), &prefs("es", "ja"));
        assert_eq!((sid, aid), (3, 2));
        // No matching track: mpv's own choice stays.
        let (sid, aid) = on_tracks(None, &list, &SubtitlePrefs::default(), &prefs("pt", "es"));
        assert_eq!((sid, aid), (1, 1));
        assert_eq!(on_tracks(None, &[], &SubtitlePrefs::default(), &SourcePrefsVm::default()), (0, 0));
    }

    #[test]
    fn secondary_never_reuses_the_primary_track() {
        let list = vec![tr(1, TrackKind::Sub, "en", None, false), tr(2, TrackKind::Sub, "es", None, false)];
        let subs = SubtitlePrefs { secondary_lang: Some("en".into()), ..SubtitlePrefs::default() };
        let c = choose(&list, &subs, &prefs("es", "ja"));
        assert_eq!((c.sub, c.secondary), (Some(2), Some(1)));
        let same = SubtitlePrefs { secondary_lang: Some("es".into()), ..SubtitlePrefs::default() };
        assert_eq!(choose(&list, &same, &prefs("es", "ja")).secondary, None);
    }

    #[test]
    fn style_maps_to_mpv_units() {
        let s = SubtitleStyleVm { size: 150, font: " Outfit ".into(), outline: 3, outline_color: "#112233".into(), bg_opacity: 40, ..SubtitleStyleVm::default() };
        let m = to_sub_style(&s);
        assert_eq!((m.scale, m.font.as_deref(), m.border_px, m.border_color.as_str(), m.back_alpha), (1.5, Some("Outfit"), 3.0, "#112233", 40));
        assert_eq!(to_sub_style(&SubtitleStyleVm::default()).font, None);
        assert!(line_style(&s, 20.0).contains("font-size:30.0px"));
    }

    fn row(id: i64, lang: &str, file: Option<&str>, codec: &str) -> TrackRow {
        TrackRow { info: tr(id, TrackKind::Sub, lang, None, false), external: file.is_some(), filename: file.map(str::to_owned), codec: Some(codec.to_owned()) }
    }

    #[test]
    fn pretranslation_picks_a_foreign_external_text_track() {
        let dir = Path::new("/data/subs");
        let mut o = Observed { tracks: vec![row(1, "ja", None, "ass"), row(2, "en", Some("https://x/en.vtt"), "webvtt"), row(3, "es", Some("https://x/es.vtt"), "webvtt")], sid: 1, ..Observed::default() };
        assert_eq!(pretranslate_source(&o, "es", dir).unwrap().info.id, 2);
        // Only target-language text: nothing to do.
        o.tracks.remove(1);
        assert_eq!(pretranslate_source(&o, "es", dir).unwrap_err(), true);
        // Embedded / bitmap / our own output only: unsupported.
        o.tracks = vec![row(1, "en", None, "ass"), row(4, "en", Some("/x/a.sup"), "hdmv_pgs_subtitle"), row(5, "en", Some("/data/subs/21-e1-es.srt"), "subrip")];
        assert_eq!(pretranslate_source(&o, "es", dir).unwrap_err(), false);
    }

    #[test]
    fn dataset_reports_the_real_selection() {
        let o = Observed {
            tracks: vec![
                TrackRow { info: tr(1, TrackKind::Audio, "jpn", None, true), external: false, filename: None, codec: None },
                row(2, "es-419", None, "ass"),
                row(3, "eng", None, "ass"),
            ],
            sid: 2,
            aid: 1,
            secondary: 3,
            delay_ms: 100,
            path: "x".into(),
        };
        let d = dataset(&o, &SubtitleStyleVm { size: 120, ..SubtitleStyleVm::default() }, true, Some("/p/a.png"));
        assert_eq!(d["subLang"], "es");
        assert_eq!(d["audioLang"], "ja");
        assert_eq!(d["secondaryLang"], "en");
        assert_eq!(d["subDelayMs"], "100");
        assert_eq!(d["subScale"], "1.20");
        assert_eq!(d["translated"], "true");
        assert_eq!(d["lastCapture"], "/p/a.png");
        let off = dataset(&Observed::default(), &SubtitleStyleVm::default(), false, None);
        assert_eq!((off["subLang"].as_str(), off["secondaryLang"].as_str()), (Some("off"), Some("off")));
        assert!(mirror_js(&d).contains("window.__tkSubs.attrs={"));
    }

    #[test]
    fn helpers() {
        assert!(is_image_codec(Some("hdmv_pgs_subtitle")) && !is_image_codec(Some("subrip")) && !is_image_codec(None));
        assert!(is_fetchable("https://a/b.vtt") && !is_fetchable("memory://x") && !is_fetchable("edl://x"));
        assert_eq!(file_stem("anilist:21", 3, "es"), "anilist_21-e3-es");
        assert!(is_ours("/data/subs/x.srt", Path::new("/data/subs")));
    }
}
