//! Pure UI state and reducers - no Dioxus, fully unit-tested. Screens keep these in signals and
//! feed them actions; nothing in here touches the DOM, the backend or the clock (callers pass `now`).

use serde::{Deserialize, Serialize};
use tk_player::ShaderPreset;

use crate::i18n::LangChoice;

// ---------------------------------------------------------------------------------------------
// Appearance / settings
// ---------------------------------------------------------------------------------------------

/// A curated palette. The id is the `data-palette` value in `tokens.css`; `seed` is only used for the
/// swatch in Settings (the real colors come from the tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteInfo {
    pub id: &'static str,
    pub seed: &'static str,
}

/// The 10 palettes of docs/redesign/01 section 2 (ids match `packages/tokens/tokens.json`).
pub const PALETTES: [PaletteInfo; 10] = [
    PaletteInfo { id: "tatakai-violet", seed: "#7C4DFF" },
    PaletteInfo { id: "midnight", seed: "#3D5AFE" },
    PaletteInfo { id: "sakura", seed: "#E86A92" },
    PaletteInfo { id: "matcha", seed: "#5E9E6E" },
    PaletteInfo { id: "ocean", seed: "#0091C2" },
    PaletteInfo { id: "neon-tokyo", seed: "#FF2E88" },
    PaletteInfo { id: "blood-moon", seed: "#C62828" },
    PaletteInfo { id: "royal-gold", seed: "#C9A227" },
    PaletteInfo { id: "aurora", seed: "#00BFA5" },
    PaletteInfo { id: "mono", seed: "#808080" },
];

pub const DEFAULT_PALETTE: &str = "tatakai-violet";
/// Metadata providers known to tk-core (default priority order).
pub const PROVIDERS: [&str; 3] = ["anilist", "kitsu", "anizip"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
    Amoled,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 4] = [Self::System, Self::Light, Self::Dark, Self::Amoled];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Amoled => "amoled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Contrast {
    #[default]
    Standard,
    Medium,
    High,
}

impl Contrast {
    pub const ALL: [Contrast; 3] = [Self::Standard, Self::Medium, Self::High];

    pub fn id(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Persisted user settings (the app stores this JSON in the local store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub palette: String,
    pub mode: ThemeMode,
    pub contrast: Contrast,
    pub language: LangChoice,
    /// Metadata provider ids, most preferred first.
    pub provider_priority: Vec<String>,
    /// `ShaderPreset::id()` applied when playback starts.
    pub shader_preset: String,
    pub volume: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            palette: DEFAULT_PALETTE.to_owned(),
            mode: ThemeMode::System,
            contrast: Contrast::Standard,
            language: LangChoice::System,
            provider_priority: PROVIDERS.iter().map(|s| (*s).to_owned()).collect(),
            shader_preset: ShaderPreset::Off.id().to_owned(),
            volume: 100,
        }
    }
}

/// The attributes set on `<html>`: `data-palette`, `data-mode`, `data-contrast` (absent for Standard).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeAttrs {
    pub palette: String,
    pub mode: &'static str,
    pub contrast: Option<&'static str>,
}

/// `System` follows the OS; the rest are literal.
pub fn resolve_mode(mode: ThemeMode, system_dark: bool) -> &'static str {
    match mode {
        ThemeMode::System => {
            if system_dark {
                "dark"
            } else {
                "light"
            }
        }
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::Amoled => "amoled",
    }
}

impl Settings {
    pub fn theme_attrs(&self, system_dark: bool) -> ThemeAttrs {
        ThemeAttrs {
            palette: self.palette.clone(),
            mode: resolve_mode(self.mode, system_dark),
            contrast: match self.contrast {
                Contrast::Standard => None,
                Contrast::Medium => Some("medium"),
                Contrast::High => Some("high"),
            },
        }
    }

    pub fn preset(&self) -> ShaderPreset {
        ShaderPreset::from_id(&self.shader_preset).unwrap_or_default()
    }

    /// Repair values read from an old or hand-edited file.
    pub fn sanitized(mut self) -> Self {
        if !PALETTES.iter().any(|p| p.id == self.palette) {
            self.palette = DEFAULT_PALETTE.to_owned();
        }
        self.provider_priority = sanitize_priority(&self.provider_priority);
        if ShaderPreset::from_id(&self.shader_preset).is_none() {
            self.shader_preset = ShaderPreset::Off.id().to_owned();
        }
        self.volume = self.volume.min(100);
        self
    }
}

/// Drop unknown/duplicate providers and append the missing ones in default order.
pub fn sanitize_priority(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in list {
        if PROVIDERS.contains(&p.as_str()) && !out.contains(p) {
            out.push(p.clone());
        }
    }
    for p in PROVIDERS {
        if !out.iter().any(|o| o == p) {
            out.push(p.to_owned());
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SetPalette(String),
    SetMode(ThemeMode),
    SetContrast(Contrast),
    SetLanguage(LangChoice),
    /// Move the provider at `index` by `delta` positions (negative = higher priority).
    MoveProvider { index: usize, delta: i8 },
    SetPreset(ShaderPreset),
    SetVolume(u8),
}

pub fn reduce_settings(s: &Settings, action: SettingsAction) -> Settings {
    let mut n = s.clone();
    match action {
        SettingsAction::SetPalette(id) => {
            if PALETTES.iter().any(|p| p.id == id) {
                n.palette = id;
            }
        }
        SettingsAction::SetMode(m) => n.mode = m,
        SettingsAction::SetContrast(c) => n.contrast = c,
        SettingsAction::SetLanguage(l) => n.language = l,
        SettingsAction::MoveProvider { index, delta } => {
            let len = n.provider_priority.len();
            let to = index as i64 + i64::from(delta);
            if index < len && (0..len as i64).contains(&to) {
                let item = n.provider_priority.remove(index);
                n.provider_priority.insert(to as usize, item);
            }
        }
        SettingsAction::SetPreset(p) => n.shader_preset = p.id().to_owned(),
        SettingsAction::SetVolume(v) => n.volume = v.min(100),
    }
    n
}

// ---------------------------------------------------------------------------------------------
// Navigation / layout
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavDest {
    Home,
    Search,
    Extensions,
    Link,
    Settings,
}

impl NavDest {
    pub const ALL: [NavDest; 5] = [Self::Home, Self::Search, Self::Extensions, Self::Link, Self::Settings];

    /// i18n key of the label (`nav.<id>`).
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Home => "nav.home",
            Self::Search => "nav.search",
            Self::Extensions => "nav.extensions",
            Self::Link => "nav.link",
            Self::Settings => "nav.settings",
        }
    }
}

/// Material window size classes (dp == CSS px).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowClass {
    Compact,
    Medium,
    Expanded,
}

pub fn window_class(width: f64) -> WindowClass {
    if width < 840.0 {
        WindowClass::Compact
    } else if width < 1200.0 {
        WindowClass::Medium
    } else {
        WindowClass::Expanded
    }
}

/// Multi-pane arrangement: Search is list | detail, Details is info | episodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneLayout {
    Single,
    Dual,
}

pub fn pane_layout(width: f64) -> PaneLayout {
    if window_class(width) == WindowClass::Compact { PaneLayout::Single } else { PaneLayout::Dual }
}

/// Rail (collapsed) / drawer (expanded). Ctrl+B toggles; compact windows force collapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NavState {
    pub expanded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    Toggle,
    Collapse,
}

pub fn reduce_nav(s: NavState, a: NavAction) -> NavState {
    match a {
        NavAction::Toggle => NavState { expanded: !s.expanded },
        NavAction::Collapse => NavState { expanded: false },
    }
}

/// Effective expansion: a compact window never shows the drawer.
pub fn nav_expanded(s: NavState, width: f64) -> bool {
    s.expanded && window_class(width) != WindowClass::Compact
}

// ---------------------------------------------------------------------------------------------
// Async loading
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Offline,
    RateLimited,
    Extension,
    NoSource,
    Auth,
    Other,
}

impl ErrorKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Offline => "error.offline",
            Self::RateLimited => "error.rate_limited",
            Self::Extension => "error.extension",
            Self::NoSource => "error.no_source",
            Self::Auth => "error.auth",
            Self::Other => "error.generic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError {
    pub kind: ErrorKind,
    /// Technical detail (provider name, message); shown small under the localized text.
    pub detail: String,
}

impl UiError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadState<T> {
    Loading,
    Ready(T),
    Failed(UiError),
}

impl<T> LoadState<T> {
    pub fn from_result(r: Result<T, UiError>) -> Self {
        match r {
            Ok(v) => Self::Ready(v),
            Err(e) => Self::Failed(e),
        }
    }
}

/// Skeletons only appear after this delay (docs/redesign/02 section 5).
pub const SKELETON_DELAY_MS: u64 = 150;

// ---------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------

pub const MIN_QUERY_CHARS: usize = 2;
pub const SEARCH_DEBOUNCE_MS: u64 = 350;

pub fn normalize_query(q: &str) -> String {
    q.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn query_is_searchable(q: &str) -> bool {
    normalize_query(q).chars().count() >= MIN_QUERY_CHARS
}

/// Generation counter that drops stale responses: each keystroke bumps it; only the response whose
/// generation still matches is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchState<T> {
    pub query: String,
    pub generation: u64,
    pub results: Option<LoadState<Vec<T>>>,
}

impl<T> Default for SearchState<T> {
    fn default() -> Self {
        Self { query: String::new(), generation: 0, results: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchAction<T> {
    Typed(String),
    Started(u64),
    Finished { generation: u64, result: Result<Vec<T>, UiError> },
}

pub fn reduce_search<T: Clone>(s: &SearchState<T>, a: SearchAction<T>) -> SearchState<T> {
    let mut n = s.clone();
    match a {
        SearchAction::Typed(q) => {
            n.generation += 1;
            n.query = q;
            if !query_is_searchable(&n.query) {
                n.results = None;
            }
        }
        SearchAction::Started(g) => {
            if g == n.generation {
                n.results = Some(LoadState::Loading);
            }
        }
        SearchAction::Finished { generation, result } => {
            if generation == n.generation && query_is_searchable(&n.query) {
                n.results = Some(LoadState::from_result(result));
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------------------------

pub const EPISODE_RANGE: u32 = 100;

/// Inclusive `(first, last)` ranges of up to `size` episodes: `episode_ranges(250, 100)` = 1-100, 101-200, 201-250.
pub fn episode_ranges(total: u32, size: u32) -> Vec<(u32, u32)> {
    let size = size.max(1);
    (0..total.div_ceil(size)).map(|i| (i * size + 1, ((i + 1) * size).min(total))).collect()
}

/// The range containing `episode`, so the tab with the current episode opens first.
pub fn range_index_for(episode: u32, size: u32) -> usize {
    (episode.max(1) - 1) as usize / size.max(1) as usize
}

// ---------------------------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------------------------

pub const CONTROLS_HIDE_MS: u64 = 3000;
pub const PROGRESS_REPORT_MS: u64 = 15_000;
pub const SEEK_STEP_SECS: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub buffering: bool,
    /// mpv's video output is configured: the hole may open.
    pub video_ready: bool,
    pub ended: bool,
    pub error: Option<String>,
    pub last_activity_ms: u64,
    pub volume: u8,
    pub muted: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            position: 0.0,
            duration: 0.0,
            paused: false,
            buffering: true,
            video_ready: false,
            ended: false,
            error: None,
            last_activity_ms: 0,
            volume: 100,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerAction {
    Position(f64),
    Duration(f64),
    Paused(bool),
    Buffering(bool),
    VideoReady(bool),
    Eof,
    Error(String),
    /// Mouse move / key press.
    Activity(u64),
    SetVolume(u8),
    ToggleMute,
    /// A new stream is being loaded (resets position/error/ended).
    Loading,
}

pub fn reduce_player(s: &PlayerState, a: PlayerAction) -> PlayerState {
    let mut n = s.clone();
    match a {
        PlayerAction::Position(p) => {
            if p.is_finite() {
                n.position = p.max(0.0);
                n.buffering = false;
            }
        }
        PlayerAction::Duration(d) => {
            if d.is_finite() {
                n.duration = d.max(0.0);
            }
        }
        PlayerAction::Paused(p) => n.paused = p,
        PlayerAction::Buffering(b) => n.buffering = b,
        PlayerAction::VideoReady(v) => n.video_ready = v,
        PlayerAction::Eof => {
            n.ended = true;
            n.paused = true;
        }
        PlayerAction::Error(e) => n.error = Some(e),
        PlayerAction::Activity(now) => n.last_activity_ms = now,
        PlayerAction::SetVolume(v) => {
            n.volume = v.min(100);
            n.muted = false;
        }
        PlayerAction::ToggleMute => n.muted = !n.muted,
        PlayerAction::Loading => {
            n.position = 0.0;
            n.duration = 0.0;
            n.paused = false;
            n.buffering = true;
            n.video_ready = false;
            n.ended = false;
            n.error = None;
        }
    }
    n
}

impl PlayerState {
    /// Controls stay up while paused, on error, or for [`CONTROLS_HIDE_MS`] after the last activity.
    pub fn controls_visible(&self, now_ms: u64) -> bool {
        self.paused || self.error.is_some() || self.ended || now_ms.saturating_sub(self.last_activity_ms) < CONTROLS_HIDE_MS
    }

    /// 0.0..=1.0
    pub fn fraction(&self) -> f64 {
        if self.duration > 0.0 { (self.position / self.duration).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// Absolute seconds for a click at `fraction` of the seek bar.
    pub fn seek_target(&self, fraction: f64) -> f64 {
        fraction.clamp(0.0, 1.0) * self.duration
    }

    pub fn progress_secs(&self) -> u32 {
        self.position.max(0.0) as u32
    }

    pub fn duration_secs(&self) -> Option<u32> {
        (self.duration > 0.0).then_some(self.duration as u32)
    }
}

/// Whether enough time passed to report progress to the backend again (every 15 s of wall clock).
pub fn should_report(last_report_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(last_report_ms) >= PROGRESS_REPORT_MS
}

/// `75` -> `1:15`, `3725` -> `1:02:05`; negative/NaN -> `0:00`.
pub fn format_time(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 { secs as u64 } else { 0 };
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") }
}

pub fn next_episode(current: u32, total: Option<u32>) -> Option<u32> {
    match total {
        Some(t) if current < t => Some(current + 1),
        Some(_) => None,
        // Unknown total (airing): optimistic; the backend reports "no source" if it does not exist.
        None => Some(current + 1),
    }
}

// ---------------------------------------------------------------------------------------------
// Link pairing
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub uri: String,
    pub code: String,
    /// Unix seconds when the 5-minute window closes.
    pub expires_unix: i64,
}

impl Pairing {
    pub fn remaining_secs(&self, now_unix: i64) -> i64 {
        (self.expires_unix - now_unix).max(0)
    }

    pub fn is_expired(&self, now_unix: i64) -> bool {
        self.remaining_secs(now_unix) == 0
    }
}

/// `m:ss`
pub fn format_countdown(remaining_secs: i64) -> String {
    let r = remaining_secs.max(0);
    format!("{}:{:02}", r / 60, r % 60)
}

/// `ABCDEFGHJK` -> `ABCDE-FGHJK` (groups of 5; the Crockford code is 10 chars).
pub fn format_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    chars.chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// `1_700_000_000` -> `2023-11-14 22:13 UTC` (no time-zone database needed).
pub fn format_unix_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Days since 1970-01-01 -> civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem / 60) % 60)
}

// ---------------------------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatLevel {
    Full,
    Partial,
    Unsupported,
}

impl CompatLevel {
    /// CSS modifier and i18n suffix: `compat.<id>`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
        }
    }

    /// Unsupported extensions cannot be enabled.
    pub fn can_enable(self) -> bool {
        self != Self::Unsupported
    }
}

/// Accepts only `.kai` packages (case-insensitive).
pub fn is_kai_path(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && ext.eq_ignore_ascii_case("kai"))
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, lookup};

    #[test]
    fn defaults_are_valid_and_survive_sanitize() {
        let d = Settings::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(d.theme_attrs(true).mode, "dark");
        assert_eq!(d.theme_attrs(false).mode, "light");
        assert_eq!(d.theme_attrs(true).contrast, None);
    }

    #[test]
    fn theme_attrs_follow_mode_and_contrast() {
        let s = Settings { mode: ThemeMode::Amoled, contrast: Contrast::High, palette: "sakura".into(), ..Settings::default() };
        let a = s.theme_attrs(false);
        assert_eq!((a.palette.as_str(), a.mode, a.contrast), ("sakura", "amoled", Some("high")));
        let s = reduce_settings(&s, SettingsAction::SetContrast(Contrast::Medium));
        assert_eq!(s.theme_attrs(false).contrast, Some("medium"));
    }

    #[test]
    fn unknown_palette_is_rejected_and_repaired() {
        let s = reduce_settings(&Settings::default(), SettingsAction::SetPalette("nope".into()));
        assert_eq!(s.palette, DEFAULT_PALETTE);
        let s = reduce_settings(&Settings::default(), SettingsAction::SetPalette("ocean".into()));
        assert_eq!(s.palette, "ocean");
        let broken = Settings { palette: "x".into(), shader_preset: "y".into(), volume: 250, ..Settings::default() }.sanitized();
        assert_eq!((broken.palette.as_str(), broken.shader_preset.as_str(), broken.volume), (DEFAULT_PALETTE, "off", 100));
    }

    #[test]
    fn provider_priority_reorders_within_bounds() {
        let s = Settings::default();
        assert_eq!(s.provider_priority, ["anilist", "kitsu", "anizip"]);
        let s = reduce_settings(&s, SettingsAction::MoveProvider { index: 2, delta: -1 });
        assert_eq!(s.provider_priority, ["anilist", "anizip", "kitsu"]);
        let same = reduce_settings(&s, SettingsAction::MoveProvider { index: 0, delta: -1 });
        assert_eq!(same, s);
        let same = reduce_settings(&s, SettingsAction::MoveProvider { index: 2, delta: 1 });
        assert_eq!(same, s);
        let same = reduce_settings(&s, SettingsAction::MoveProvider { index: 9, delta: -1 });
        assert_eq!(same, s);
    }

    #[test]
    fn sanitize_priority_dedupes_and_completes() {
        let l = sanitize_priority(&["kitsu".into(), "bogus".into(), "kitsu".into()]);
        assert_eq!(l, ["kitsu", "anilist", "anizip"]);
    }

    #[test]
    fn settings_json_round_trips_and_tolerates_partial_files() {
        let s = Settings { mode: ThemeMode::Dark, language: LangChoice::Es, ..Settings::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let partial: Settings = serde_json::from_str(r#"{"palette":"mono"}"#).unwrap();
        assert_eq!(partial.palette, "mono");
        assert_eq!(partial.mode, ThemeMode::System);
    }

    #[test]
    fn dynamic_i18n_keys_exist_in_both_languages() {
        for lang in [Lang::En, Lang::Es] {
            for p in PALETTES {
                let k = format!("palette.{}", p.id);
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for m in ThemeMode::ALL {
                let k = format!("mode.{}", m.id());
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for c in Contrast::ALL {
                let k = format!("contrast.{}", c.id());
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for l in LangChoice::ALL {
                let k = format!("lang.{}", l.id());
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for p in ShaderPreset::ALL {
                let k = format!("preset.{}", p.id());
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for p in PROVIDERS {
                let k = format!("provider.{p}");
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for c in [CompatLevel::Full, CompatLevel::Partial, CompatLevel::Unsupported] {
                let k = format!("compat.{}", c.id());
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
            for d in NavDest::ALL {
                assert_ne!(lookup(lang, d.label_key()), d.label_key());
            }
            for k in [ErrorKind::Offline, ErrorKind::RateLimited, ErrorKind::Extension, ErrorKind::NoSource, ErrorKind::Auth, ErrorKind::Other] {
                assert_ne!(lookup(lang, k.key()), k.key());
            }
        }
    }

    #[test]
    fn nav_toggle_and_compact_override() {
        let s = reduce_nav(NavState::default(), NavAction::Toggle);
        assert!(s.expanded);
        assert!(nav_expanded(s, 1400.0));
        assert!(!nav_expanded(s, 700.0));
        assert!(!reduce_nav(s, NavAction::Collapse).expanded);
        assert!(!reduce_nav(s, NavAction::Toggle).expanded);
    }

    #[test]
    fn window_classes_and_panes() {
        assert_eq!(window_class(600.0), WindowClass::Compact);
        assert_eq!(window_class(840.0), WindowClass::Medium);
        assert_eq!(window_class(1200.0), WindowClass::Expanded);
        assert_eq!(pane_layout(800.0), PaneLayout::Single);
        assert_eq!(pane_layout(1000.0), PaneLayout::Dual);
    }

    #[test]
    fn search_drops_stale_responses() {
        let s0: SearchState<u32> = SearchState::default();
        let s1 = reduce_search(&s0, SearchAction::Typed("na".into()));
        let g1 = s1.generation;
        let s2 = reduce_search(&s1, SearchAction::Typed("nar".into()));
        // The response for "na" arrives after "nar" was typed: ignored.
        let s3 = reduce_search(&s2, SearchAction::Finished { generation: g1, result: Ok(vec![1]) });
        assert_eq!(s3.results, None);
        let s4 = reduce_search(&s3, SearchAction::Finished { generation: s2.generation, result: Ok(vec![2]) });
        assert_eq!(s4.results, Some(LoadState::Ready(vec![2])));
        // Clearing the query clears the results and invalidates in-flight requests.
        let s5 = reduce_search(&s4, SearchAction::Typed("n".into()));
        assert_eq!(s5.results, None);
        let s6 = reduce_search(&s5, SearchAction::Finished { generation: s4.generation, result: Ok(vec![3]) });
        assert_eq!(s6.results, None);
    }

    #[test]
    fn search_errors_are_kept() {
        let s = reduce_search(&SearchState::<u32>::default(), SearchAction::Typed("abc".into()));
        let e = UiError::new(ErrorKind::Offline, "x");
        let s = reduce_search(&s, SearchAction::Finished { generation: s.generation, result: Err(e.clone()) });
        assert_eq!(s.results, Some(LoadState::Failed(e)));
    }

    #[test]
    fn query_normalization() {
        assert_eq!(normalize_query("  one   punch  man "), "one punch man");
        assert!(!query_is_searchable(" a "));
        assert!(query_is_searchable("ab"));
    }

    #[test]
    fn episode_ranges_group_by_hundreds() {
        assert_eq!(episode_ranges(250, 100), [(1, 100), (101, 200), (201, 250)]);
        assert_eq!(episode_ranges(12, 100), [(1, 12)]);
        assert_eq!(episode_ranges(0, 100), []);
        assert_eq!(range_index_for(1, 100), 0);
        assert_eq!(range_index_for(100, 100), 0);
        assert_eq!(range_index_for(101, 100), 1);
    }

    #[test]
    fn player_reducer_tracks_playback() {
        let mut s = PlayerState::default();
        assert!(s.buffering && !s.video_ready);
        s = reduce_player(&s, PlayerAction::Duration(1400.0));
        s = reduce_player(&s, PlayerAction::Position(350.0));
        s = reduce_player(&s, PlayerAction::VideoReady(true));
        assert!(!s.buffering && s.video_ready);
        assert!((s.fraction() - 0.25).abs() < 1e-9);
        assert_eq!((s.progress_secs(), s.duration_secs()), (350, Some(1400)));
        assert_eq!(s.seek_target(0.5), 700.0);
        assert_eq!(s.seek_target(2.0), 1400.0);
        s = reduce_player(&s, PlayerAction::Eof);
        assert!(s.ended && s.paused);
        s = reduce_player(&s, PlayerAction::Loading);
        assert_eq!((s.position, s.duration), (0.0, 0.0));
        assert!(!s.ended && !s.video_ready && s.buffering);
    }

    #[test]
    fn player_ignores_non_finite_values() {
        let s = reduce_player(&PlayerState::default(), PlayerAction::Position(f64::NAN));
        assert_eq!(s.position, 0.0);
        let s = reduce_player(&s, PlayerAction::Duration(f64::INFINITY));
        assert_eq!(s.duration, 0.0);
        assert_eq!(s.fraction(), 0.0);
        assert_eq!(s.duration_secs(), None);
    }

    #[test]
    fn controls_auto_hide_unless_paused_or_failed() {
        let mut s = reduce_player(&PlayerState::default(), PlayerAction::Activity(1000));
        assert!(s.controls_visible(1000 + CONTROLS_HIDE_MS - 1));
        assert!(!s.controls_visible(1000 + CONTROLS_HIDE_MS));
        s = reduce_player(&s, PlayerAction::Paused(true));
        assert!(s.controls_visible(999_999));
        s = reduce_player(&s, PlayerAction::Paused(false));
        s = reduce_player(&s, PlayerAction::Error("boom".into()));
        assert!(s.controls_visible(999_999));
    }

    #[test]
    fn volume_and_mute() {
        let s = reduce_player(&PlayerState::default(), PlayerAction::ToggleMute);
        assert!(s.muted);
        let s = reduce_player(&s, PlayerAction::SetVolume(180));
        assert_eq!((s.volume, s.muted), (100, false));
    }

    #[test]
    fn progress_is_reported_every_fifteen_seconds() {
        assert!(!should_report(0, 14_999));
        assert!(should_report(0, 15_000));
        assert!(!should_report(20_000, 10_000));
    }

    #[test]
    fn time_formatting() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(75.4), "1:15");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "0:00");
        assert_eq!(format_time(f64::NAN), "0:00");
    }

    #[test]
    fn next_episode_rules() {
        assert_eq!(next_episode(3, Some(12)), Some(4));
        assert_eq!(next_episode(12, Some(12)), None);
        assert_eq!(next_episode(5, None), Some(6));
    }

    #[test]
    fn pairing_countdown() {
        let p = Pairing { uri: "tatakai-link:v1?h=1".into(), code: "ABCDEFGHJK".into(), expires_unix: 1_000 + 300 };
        assert_eq!(p.remaining_secs(1_000), 300);
        assert_eq!(format_countdown(p.remaining_secs(1_000)), "5:00");
        assert_eq!(format_countdown(p.remaining_secs(1_241)), "0:59");
        assert!(!p.is_expired(1_299));
        assert!(p.is_expired(1_300));
        assert!(p.is_expired(9_999));
        assert_eq!(format_countdown(-5), "0:00");
    }

    #[test]
    fn unix_time_formatting() {
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_unix_utc(1_700_000_000), "2023-11-14 22:13 UTC");
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn pairing_code_grouping() {
        assert_eq!(format_code("ABCDEFGHJK"), "ABCDE-FGHJK");
        assert_eq!(format_code("abcde-fghjk"), "abcde-fghjk");
        assert_eq!(format_code(" ABC "), "ABC");
        assert_eq!(format_code(""), "");
    }

    #[test]
    fn kai_path_detection() {
        assert!(is_kai_path("C:\\x\\my.ext.kai"));
        assert!(is_kai_path("a.KAI"));
        assert!(!is_kai_path("a.zip"));
        assert!(!is_kai_path(".kai"));
        assert!(!is_kai_path("kai"));
    }

    #[test]
    fn compat_gates_enabling() {
        assert!(CompatLevel::Full.can_enable());
        assert!(CompatLevel::Partial.can_enable());
        assert!(!CompatLevel::Unsupported.can_enable());
    }
}
