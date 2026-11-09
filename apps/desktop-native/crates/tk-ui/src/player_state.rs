//! Player flow state (wave 1 E, docs/redesign/08): Resolving / Buffering / Playing / Error / Manual, source
//! failover over the ranked `SourceVm` list and the `failed[]` record. Pure and unit-tested like `model`;
//! no Dioxus, no backend, no clock (callers pass `now`). Reserved here so E never edits `lib.rs`.
//!
//! The screen drives it like this: after every transition it asks [`FlowState::to_resolve`]; `Some(i)`
//! means "resolve `sources[i]` now" (`Backend::playback_source`), tagged with [`FlowState::attempt`] so a
//! late answer for an older attempt is ignored. Nothing else in here starts network work.

use tk_player::{TrackInfo, TrackKind};

use crate::backend::{SkipKindVm, SkipMarkVm, SourceKindVm, SourceVm};
use crate::model::{ErrorKind, SourceModeVm, SourcePrefsVm, UiError};

/// Legacy path (`Backend::sources` not wired): at most this many `PlaybackVm::streams` are tried.
pub const MAX_LEGACY_STREAMS: usize = 3;
/// Next-episode card countdown when no setting says otherwise (`Settings::flow.auto_next_secs` wins).
pub const NEXT_COUNTDOWN_SECS: u64 = 10;
/// Short confirmations over the video ("Off", "1.5×") disappear after this.
pub const FLASH_MS: u64 = 2600;
/// Speed menu and `SpeedUp` / `SpeedDown` steps (classic `SpeedSelector.tsx`: 0.5x to 2x).
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// `VolumeUp` / `VolumeDown` step, in percent.
pub const VOLUME_STEP: u8 = 5;
/// `SubDelayEarlier` / `SubDelayLater` step (100 ms, like the classic `Z` / `X`).
pub const SUB_DELAY_STEP_SECS: f64 = 0.1;
/// Sleep timer menu (classic `usePlayerEnhancements.ts`), in minutes; "end of episode" is a separate entry.
pub const SLEEP_MINUTES: [u8; 4] = [15, 30, 45, 60];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Asking the extension for a playable URL of `sources[current]`.
    #[default]
    Resolving,
    /// The stream is loaded into mpv and has not produced a frame yet (or stalls while playing).
    Buffering,
    Playing,
    /// Nothing left to try (`error` says why, `failed` lists every attempt).
    Error,
    /// Manual mode without a pick: waiting for the user, nothing is resolved.
    Manual,
}

/// One entry of `failed[]`.
#[derive(Debug, Clone, PartialEq)]
pub struct FailedSource {
    pub id: String,
    /// `Ext · Server`.
    pub name: String,
    pub kind: ErrorKind,
    pub detail: String,
}

/// `Ext · Server` / `Ext · Group`, as in the picker rows.
pub fn source_name(s: &SourceVm) -> String {
    format!("{} · {}", s.ext_name, s.label)
}

/// What Auto mode may try on its own: any source of an enabled extension. Torrents play since wave 3
/// (docs/redesign/10 section 5.5): the ranking alone decides whether a web or a torrent source goes first.
pub fn playable(s: &SourceVm) -> bool {
    s.ext_enabled
}

/// `lang` is in `list` (`es` also matches `es-419` / `es-ES`), ignoring case.
fn has_lang(list: &[String], lang: &str) -> bool {
    let lang = lang.trim();
    !lang.is_empty()
        && list.iter().any(|l| {
            let l = l.trim();
            l.eq_ignore_ascii_case(lang)
                || (l.len() > lang.len() && l.is_char_boundary(lang.len()) && l[..lang.len()].eq_ignore_ascii_case(lang) && l[lang.len()..].starts_with(['-', '_']))
        })
}

/// The source still fits the current preferences (wave A, worker e; audit bug `sources.remembered-vs-prefs`):
/// its quality is unknown or at least `min_quality`, and either it is dubbed in the subtitle language (nothing to
/// read) or its audio is `dub_lang` (or unknown) **and** it carries `sub_lang` subtitles. A Russian torrent without
/// subtitles fails for "JA audio + ES subtitles"; a JA source with ES subtitles passes.
pub fn meets_prefs(s: &SourceVm, prefs: &SourcePrefsVm) -> bool {
    let rank = quality_rank(&s.quality);
    let quality_ok = rank == 0 || rank >= quality_rank(prefs.min_quality.id());
    let dubbed_in_reading_lang = has_lang(&s.dubs, &prefs.sub_lang);
    let audio_ok = s.dubs.is_empty() || has_lang(&s.dubs, &prefs.dub_lang);
    let subs_ok = has_lang(&s.subs, &prefs.sub_lang);
    quality_ok && (dubbed_in_reading_lang || (audio_ok && subs_ok))
}

/// Forget a remembered pick that no longer fits the preferences, as long as some other playable source does
/// (when nothing fits, the user's choice is no worse than the ranking's and it stays). Clears `picked` in place
/// and returns the id that was dropped, so the screen also forgets it in the store (`Backend::set_picked_source`).
pub fn drop_stale_pick(sources: &mut [SourceVm], prefs: &SourcePrefsVm) -> Option<String> {
    let i = sources.iter().position(|s| s.picked)?;
    if meets_prefs(&sources[i], prefs) {
        return None;
    }
    if !sources.iter().enumerate().any(|(j, s)| j != i && playable(s) && meets_prefs(s, prefs)) {
        return None;
    }
    let id = sources[i].id.clone();
    for s in sources.iter_mut() {
        s.picked = false;
    }
    Some(id)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowState {
    pub phase: Phase,
    pub mode: SourceModeVm,
    /// Ranked best first (`Backend::sources`); `picked` marks the user's choice.
    pub sources: Vec<SourceVm>,
    /// Index into `sources` of the source being resolved / played.
    pub current: Option<usize>,
    pub failed: Vec<FailedSource>,
    pub error: Option<UiError>,
    /// Bumped every time a source starts resolving.
    pub attempt: u64,
    /// Built from `Backend::playback` streams: no reliability reports, no picker.
    pub legacy: bool,
    /// The remembered pick that `new` ignored because it no longer meets the preferences ([`drop_stale_pick`]);
    /// the screen forgets it with `Backend::set_picked_source(None)`.
    pub dropped_pick: Option<String>,
    started: bool,
}

impl FlowState {
    /// `prefs` = `Settings::sources`: a remembered pick that no longer meets them is ignored (and reported in
    /// [`FlowState::dropped_pick`]); the recommended source plays and the old pick stays in the list as an option.
    pub fn new(mut sources: Vec<SourceVm>, mode: SourceModeVm, prefs: &SourcePrefsVm) -> Self {
        let dropped_pick = drop_stale_pick(&mut sources, prefs);
        let mut s = Self { sources, mode, dropped_pick, ..Self::default() };
        s.begin();
        s
    }

    /// A source is marked as the user's pick (`data-source-picked`).
    pub fn has_pick(&self) -> bool {
        self.sources.iter().any(|s| s.picked)
    }

    /// Sources synthesized from the old `PlaybackVm::streams` (always Auto, plain failover).
    pub fn legacy(sources: Vec<SourceVm>) -> Self {
        let mut s = Self { sources, mode: SourceModeVm::Auto, legacy: true, ..Self::default() };
        s.begin();
        s
    }

    fn begin(&mut self) {
        self.failed.clear();
        self.error = None;
        self.current = None;
        if self.sources.is_empty() {
            self.stop(UiError::new(ErrorKind::NoSource, ""));
            return;
        }
        if let Some(i) = self.sources.iter().position(|s| s.picked) {
            self.enter(i);
            return;
        }
        if self.mode == SourceModeVm::Manual && !self.legacy {
            self.phase = Phase::Manual;
            return;
        }
        match self.next_playable() {
            Some(i) => self.enter(i),
            None => {
                let e = self.no_playable_error();
                self.stop(e);
            }
        }
    }

    fn no_playable_error(&self) -> UiError {
        UiError::new(ErrorKind::NoSource, "")
    }

    fn enter(&mut self, i: usize) {
        self.current = Some(i);
        self.phase = Phase::Resolving;
        self.attempt += 1;
        self.started = false;
        self.error = None;
    }

    fn stop(&mut self, error: UiError) {
        self.phase = Phase::Error;
        self.error = Some(error);
    }

    /// First playable source in ranking order that has not failed yet.
    fn next_playable(&self) -> Option<usize> {
        (0..self.sources.len()).find(|&i| {
            let s = &self.sources[i];
            playable(s) && !self.failed.iter().any(|f| f.id == s.id)
        })
    }

    /// `Some(i)` while `sources[i]` must be resolved; `None` in every other phase.
    pub fn to_resolve(&self) -> Option<usize> {
        if self.phase == Phase::Resolving { self.current } else { None }
    }

    pub fn current_source(&self) -> Option<&SourceVm> {
        self.current.and_then(|i| self.sources.get(i))
    }

    /// `(n, total)` for "Source n of total".
    pub fn position(&self) -> Option<(usize, usize)> {
        self.current.map(|i| (i + 1, self.sources.len()))
    }

    /// The user picked `id` (picker, quality menu, "Use the best web source"). Marks it as the pick and
    /// starts resolving it, whatever the mode. Unknown ids change nothing.
    pub fn pick(&mut self, id: &str) -> bool {
        let Some(i) = self.sources.iter().position(|s| s.id == id) else {
            return false;
        };
        for s in &mut self.sources {
            s.picked = s.id == id;
        }
        self.failed.retain(|f| f.id != id);
        self.enter(i);
        true
    }

    /// The stream of `attempt` was handed to mpv.
    pub fn resolved(&mut self, attempt: u64) {
        if attempt == self.attempt && self.phase == Phase::Resolving {
            self.phase = Phase::Buffering;
        }
    }

    /// mpv shows frames. Returns the source id to report as working, once per attempt (never in legacy).
    pub fn started(&mut self) -> Option<String> {
        if !matches!(self.phase, Phase::Buffering | Phase::Playing) {
            return None;
        }
        self.phase = Phase::Playing;
        if self.started {
            return None;
        }
        self.started = true;
        if self.legacy { None } else { self.current_source().map(|s| s.id.clone()) }
    }

    /// Cache starvation while playing (`paused-for-cache`).
    pub fn stalled(&mut self, stalled: bool) {
        match (stalled, self.phase) {
            (true, Phase::Playing) => self.phase = Phase::Buffering,
            (false, Phase::Buffering) if self.started => self.phase = Phase::Playing,
            _ => {}
        }
    }

    /// The source of `attempt` failed (resolve, load or mpv error). Records it in `failed`; Auto (and
    /// legacy) moves on to the next playable source, Manual stops so the user picks again. Returns the
    /// failed source id to report (`None` for stale attempts and legacy).
    pub fn fail(&mut self, attempt: u64, error: UiError) -> Option<String> {
        if attempt != self.attempt || !matches!(self.phase, Phase::Resolving | Phase::Buffering | Phase::Playing) {
            return None;
        }
        let s = self.current_source()?.clone();
        self.failed.push(FailedSource { id: s.id.clone(), name: source_name(&s), kind: error.kind, detail: error.detail.clone() });
        let report = (!self.legacy).then(|| s.id.clone());
        if (self.mode == SourceModeVm::Auto || self.legacy)
            && let Some(n) = self.next_playable()
        {
            self.enter(n);
            return report;
        }
        self.stop(error);
        report
    }

    /// "Retry" on the final error: forget the failures and start over (the pick still wins).
    pub fn retry(&mut self) {
        self.begin();
    }

    /// Best web source to offer when a torrent fails (not failed yet if possible).
    pub fn best_web(&self) -> Option<&SourceVm> {
        let web = |s: &&SourceVm| playable(s) && s.kind == SourceKindVm::Web;
        self.sources
            .iter()
            .filter(web)
            .find(|s| !self.failed.iter().any(|f| f.id == s.id))
            .or_else(|| self.sources.iter().find(web))
    }

    /// The last failure was a torrent source (the error screen then offers the best web source).
    pub fn last_failed_torrent(&self) -> bool {
        self.failed
            .last()
            .and_then(|f| self.sources.iter().find(|s| s.id == f.id))
            .is_some_and(|s| s.kind == SourceKindVm::Torrent)
    }

    /// Distinct qualities of the playable sources, highest first (quality menu).
    pub fn qualities(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in self.sources.iter().filter(|s| playable(s) && !s.quality.is_empty()) {
            if !out.contains(&s.quality) {
                out.push(s.quality.clone());
            }
        }
        out.sort_by_key(|q| std::cmp::Reverse(quality_rank(q)));
        out
    }

    /// Best-ranked playable source with that quality (not failed yet if possible).
    pub fn source_for_quality(&self, quality: &str) -> Option<&SourceVm> {
        let all: Vec<&SourceVm> = self.sources.iter().filter(|s| playable(s) && s.quality == quality).collect();
        all.iter().find(|s| !self.failed.iter().any(|f| f.id == s.id)).or(all.first()).copied()
    }
}

/// `"1080p"` -> 1080; unknown -> 0.
pub fn quality_rank(q: &str) -> u32 {
    q.trim_end_matches(['p', 'P']).parse().unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Intro / outro / recap markers (`Backend::skip_marks`, AniSkip) and torrent seek-bar ranges
// ---------------------------------------------------------------------------------------------

/// A marked range in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Span {
    fn contains(self, pos: f64) -> bool {
        pos >= self.start && pos < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Markers {
    pub intro: Option<Span>,
    pub outro: Option<Span>,
    pub recap: Option<Span>,
}

/// One marker drawn on the seek bar: `(left %, width %)` of the bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarMark {
    pub kind: SkipKindVm,
    pub left: f64,
    pub width: f64,
}

impl Markers {
    /// The first mark of each kind (AniSkip may list more than one; the earliest wins).
    pub fn from_skips(marks: &[SkipMarkVm]) -> Self {
        let mut out = Self::default();
        for m in marks {
            if !(m.start_secs.is_finite() && m.end_secs.is_finite() && m.end_secs > m.start_secs) {
                continue;
            }
            let span = Span { start: m.start_secs.max(0.0), end: m.end_secs };
            let slot = match m.kind {
                SkipKindVm::Intro => &mut out.intro,
                SkipKindVm::Outro => &mut out.outro,
                SkipKindVm::Recap => &mut out.recap,
            };
            if slot.is_none_or(|old| span.start < old.start) {
                *slot = Some(span);
            }
        }
        out
    }

    fn spans(&self) -> [(SkipKindVm, Option<Span>); 3] {
        [(SkipKindVm::Recap, self.recap), (SkipKindVm::Intro, self.intro), (SkipKindVm::Outro, self.outro)]
    }

    /// The mark `pos` is inside and where skipping it jumps to (its end).
    pub fn skip_at(&self, pos: f64) -> Option<(SkipKindVm, f64)> {
        self.spans().into_iter().find_map(|(kind, s)| s.filter(|s| s.contains(pos)).map(|s| (kind, s.end)))
    }

    /// Where "Skip intro" jumps to, while `pos` is inside the intro.
    pub fn skip_target(&self, pos: f64) -> Option<f64> {
        self.intro.filter(|s| s.contains(pos)).map(|s| s.end)
    }

    /// `(left %, width %)` of every marker on a seek bar of `duration` seconds.
    pub fn bar_spans(&self, duration: f64) -> Vec<(f64, f64)> {
        self.bar_marks(duration).into_iter().map(|m| (m.left, m.width)).collect()
    }

    /// Every marker with its kind (the CSS colours them per kind), in recap / intro / outro order.
    pub fn bar_marks(&self, duration: f64) -> Vec<BarMark> {
        if !(duration.is_finite() && duration > 0.0) {
            return Vec::new();
        }
        self.spans()
            .into_iter()
            .filter_map(|(kind, s)| s.map(|s| (kind, s)))
            .filter(|(_, s)| s.end > s.start && s.start < duration)
            .map(|(kind, s)| {
                let a = (s.start / duration).clamp(0.0, 1.0);
                let b = (s.end / duration).clamp(0.0, 1.0);
                BarMark { kind, left: a * 100.0, width: (b - a) * 100.0 }
            })
            .collect()
    }
}

/// Downloaded ranges of the playing file (`Backend::seek_marks`, byte fractions 0.0..=1.0, sorted) as
/// `(left %, width %)` of the seek bar. Byte fractions stand in for time fractions (near-constant bitrate).
pub fn range_spans(ranges: &[(f64, f64)]) -> Vec<(f64, f64)> {
    ranges
        .iter()
        .filter(|(a, b)| a.is_finite() && b.is_finite())
        .map(|&(a, b)| (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0)))
        .filter(|(a, b)| b > a)
        .map(|(a, b)| (a * 100.0, (b - a) * 100.0))
        .collect()
}

/// Share of the file already downloaded (sum of the ranges), 0.0..=1.0.
pub fn downloaded_fraction(ranges: &[(f64, f64)]) -> f64 {
    ranges
        .iter()
        .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
        .map(|(a, b)| (b.min(1.0) - a.max(0.0)).max(0.0))
        .sum::<f64>()
        .clamp(0.0, 1.0)
}

/// "Ready to play" from `at` (fraction of the file): `(left %, width %)` from `at` to the end of the downloaded
/// range that contains it. `None` when the position is not downloaded yet (mpv is waiting for data there).
pub fn ready_span(ranges: &[(f64, f64)], at: f64) -> Option<(f64, f64)> {
    if !at.is_finite() {
        return None;
    }
    let at = at.clamp(0.0, 1.0);
    // A tiny slack: right after a seek the playhead sits on a range boundary.
    ranges
        .iter()
        .find(|(a, b)| at >= *a - 1e-6 && at < *b)
        .map(|&(_, b)| (at * 100.0, (b.min(1.0) - at).max(0.0) * 100.0))
        .filter(|(_, w)| *w > 0.0)
}

/// `4.20 MB/s`, `512 KB/s`, `0 B/s` (decimal units, like the torrent client).
pub fn format_rate(bps: u64) -> String {
    format!("{}/s", format_bytes(bps))
}

/// `13.8 GB`, `512 KB`, `4.20 MB`, `12 B`.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1000.0;
    let mut unit = 0;
    while v >= 1000.0 && unit + 1 < UNITS.len() {
        v /= 1000.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.2} {}", UNITS[unit])
    }
}

/// A torrent id / v1 info-hash: 40 hex characters.
pub fn is_info_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------------------------
// Small helpers for the controls
// ---------------------------------------------------------------------------------------------

/// Pointer `x` over a seek bar `width` px wide -> 0.0..=1.0 (seek hover preview).
pub fn hover_fraction(x: f64, width: f64) -> f64 {
    if width > 0.0 && x.is_finite() { (x / width).clamp(0.0, 1.0) } else { 0.0 }
}

/// The controls (and the status they carry: "Looking for a source", the buffering spinner) stay up regardless of
/// pointer activity until the first frame: resolving, waiting for a manual pick, or loading before any picture.
pub fn keeps_controls_up(phase: Phase, video_ready: bool) -> bool {
    match phase {
        Phase::Resolving | Phase::Manual => true,
        Phase::Buffering => !video_ready,
        Phase::Playing | Phase::Error => false,
    }
}

/// Seconds left on a `secs`-long next-episode countdown that appeared at `since_ms`.
pub fn countdown_left(since_ms: u64, now_ms: u64, secs: u64) -> u64 {
    secs.saturating_sub(now_ms.saturating_sub(since_ms) / 1000)
}

/// Auto-skip intro (`Settings::flow.auto_skip_intro`): where to jump while `pos` is inside the intro, once per
/// screen (`done` = already skipped, so seeking back into the intro lets the user watch it).
pub fn auto_skip_target(markers: &Markers, pos: f64, done: bool) -> Option<f64> {
    if done || !pos.is_finite() {
        return None;
    }
    markers.skip_target(pos)
}

/// Next speed of [`SPEEDS`] above (`up`) or below `current`; stays at the ends.
pub fn speed_step(current: f64, up: bool) -> f64 {
    let current = if current.is_finite() { current } else { 1.0 };
    if up {
        SPEEDS.iter().copied().find(|s| *s > current + 1e-6).unwrap_or(SPEEDS[SPEEDS.len() - 1])
    } else {
        SPEEDS.iter().rev().copied().find(|s| *s < current - 1e-6).unwrap_or(SPEEDS[0])
    }
}

/// `VolumeUp` / `VolumeDown`: one [`VOLUME_STEP`] up or down, within 0..=100.
pub fn volume_step(current: u8, up: bool) -> u8 {
    if up { current.saturating_add(VOLUME_STEP).min(100) } else { current.saturating_sub(VOLUME_STEP) }
}

/// `CycleSub` / `CycleAudio`: the track after `current` among the `kind` tracks, wrapping. With `with_off` the
/// cycle includes 0 (off) after the last track (subtitles); `None` when there is no track of that kind.
pub fn cycle_track(tracks: &[TrackInfo], kind: TrackKind, current: i64, with_off: bool) -> Option<i64> {
    let mut ids: Vec<i64> = tracks.iter().filter(|t| t.kind == kind).map(|t| t.id).collect();
    if ids.is_empty() {
        return None;
    }
    if with_off {
        ids.insert(0, 0);
    }
    let next = match ids.iter().position(|id| *id == current) {
        Some(i) => ids[(i + 1) % ids.len()],
        None => ids[if with_off && ids.len() > 1 { 1 } else { 0 }],
    };
    Some(next)
}

/// Sleep timer of the player screen (session state; classic `usePlayerEnhancements.ts`): when it runs out the
/// video pauses and the next episode does not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SleepTimer {
    #[default]
    Off,
    /// Pause at `deadline_ms` (`minutes` is what the user chose, for the menu).
    After { minutes: u8, deadline_ms: u64 },
    /// Pause at the end of this episode instead of playing the next one.
    EndOfEpisode,
}

/// A sleep timer menu entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepChoice {
    Off,
    Minutes(u8),
    EndOfEpisode,
}

impl SleepTimer {
    pub fn from_choice(choice: SleepChoice, now_ms: u64) -> Self {
        match choice {
            SleepChoice::Off | SleepChoice::Minutes(0) => Self::Off,
            SleepChoice::Minutes(m) => Self::After { minutes: m, deadline_ms: now_ms + u64::from(m) * 60_000 },
            SleepChoice::EndOfEpisode => Self::EndOfEpisode,
        }
    }

    /// The menu entry this timer corresponds to.
    pub fn choice(&self) -> SleepChoice {
        match self {
            Self::Off => SleepChoice::Off,
            Self::After { minutes, .. } => SleepChoice::Minutes(*minutes),
            Self::EndOfEpisode => SleepChoice::EndOfEpisode,
        }
    }

    /// A timed sleep ran out.
    pub fn due(&self, now_ms: u64) -> bool {
        matches!(self, Self::After { deadline_ms, .. } if now_ms >= *deadline_ms)
    }

    /// Whole seconds left of a timed sleep (rounded up).
    pub fn left_secs(&self, now_ms: u64) -> Option<u64> {
        match self {
            Self::After { deadline_ms, .. } => Some(deadline_ms.saturating_sub(now_ms).div_ceil(1000)),
            _ => None,
        }
    }

    pub fn is_on(&self) -> bool {
        *self != Self::Off
    }

    /// `data-sleep-timer`: `off`, `15` / `30` / `45` / `60` (minutes) or `end`.
    pub fn attr(&self) -> String {
        match self {
            Self::Off => "off".to_owned(),
            Self::After { minutes, .. } => minutes.to_string(),
            Self::EndOfEpisode => "end".to_owned(),
        }
    }
}

/// `1.25` -> `1.25×`, `2.0` -> `2×`.
pub fn format_speed(speed: f64) -> String {
    let s = format!("{speed:.2}");
    format!("{}×", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Embedded enhanced with uosc (docs/redesign/09 section 4.4): the web `KeyboardEvent.key` -> mpv key name for
/// `Player::forward_key` (`SPACE`, `LEFT`, `Ctrl+s`, `Shift+RIGHT`, `F5`). Modifier-only and unknown keys give
/// `None`. Shift is implied by printable characters (`S`, `?`) and only prefixed on named keys, like mpv does.
pub fn mpv_key_name(key: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<String> {
    let named = match key {
        " " => Some("SPACE"),
        "ArrowLeft" => Some("LEFT"),
        "ArrowRight" => Some("RIGHT"),
        "ArrowUp" => Some("UP"),
        "ArrowDown" => Some("DOWN"),
        "Enter" => Some("ENTER"),
        "Escape" => Some("ESC"),
        "Backspace" => Some("BS"),
        "Tab" => Some("TAB"),
        "Delete" => Some("DEL"),
        "Insert" => Some("INS"),
        "Home" => Some("HOME"),
        "End" => Some("END"),
        "PageUp" => Some("PGUP"),
        "PageDown" => Some("PGDWN"),
        "#" => Some("SHARP"),
        _ => None,
    };
    let (base, printable) = match named {
        Some(n) => (n.to_owned(), key == "#"),
        None if key.chars().count() == 1 => (key.to_owned(), true),
        None if key.len() <= 3 && key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
            (key.to_owned(), false)
        }
        None => return None,
    };
    let mut out = String::new();
    if ctrl {
        out.push_str("Ctrl+");
    }
    if alt {
        out.push_str("Alt+");
    }
    if meta {
        out.push_str("Meta+");
    }
    if shift && !printable {
        out.push_str("Shift+");
    }
    out.push_str(&base);
    Some(out)
}

/// Mouse button index (`MouseEvent.button`: 0 primary, 1 middle, 2 secondary) -> mpv key name.
pub fn mpv_mouse_button(button: u8) -> Option<&'static str> {
    match button {
        0 => Some("MBTN_LEFT"),
        1 => Some("MBTN_MID"),
        2 => Some("MBTN_RIGHT"),
        3 => Some("MBTN_BACK"),
        4 => Some("MBTN_FORWARD"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QualityVm;

    fn src(ext: &str, label: &str, kind: SourceKindVm, quality: &str) -> SourceVm {
        SourceVm {
            id: format!("{ext}|{}|{label}", kind.id()),
            ext_id: ext.to_owned(),
            ext_name: ext.to_owned(),
            kind,
            label: label.to_owned(),
            quality: quality.to_owned(),
            container: None,
            subs: vec!["es".into()],
            dubs: vec!["ja".into()],
            reliability: 0.8,
            note: None,
            latency_ms: Some(200),
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: false,
            picked: false,
            tags: Default::default(),
            debrid: None,
            embed: false,
        }
    }

    /// Ranked: web 1080p, torrent, web 720p, disabled extension, web 480p.
    fn list() -> Vec<SourceVm> {
        let mut v = vec![
            src("anipro", "Alpha", SourceKindVm::Web, "1080p"),
            src("nyaa", "SubsPlease", SourceKindVm::Torrent, "1080p"),
            src("anipro", "Beta", SourceKindVm::Web, "720p"),
            src("off", "Gamma", SourceKindVm::Web, "1080p"),
            src("toko", "Ok.ru", SourceKindVm::Web, "480p"),
        ];
        v[0].recommended = true;
        v[3].ext_enabled = false;
        v
    }

    /// Preferences every source of [`list`] meets (480p minimum, ES subtitles, JA audio).
    fn any() -> SourcePrefsVm {
        SourcePrefsVm { min_quality: QualityVm::P480, ..SourcePrefsVm::default() }
    }

    fn err() -> UiError {
        UiError::new(ErrorKind::Extension, "HTTP 403")
    }

    #[test]
    fn manual_mode_does_not_resolve_any_stream_until_a_source_is_picked() {
        let mut f = FlowState::new(list(), SourceModeVm::Manual, &any());
        assert_eq!(f.phase, Phase::Manual);
        assert_eq!(f.to_resolve(), None);
        assert_eq!(f.current, None);
        assert_eq!(f.attempt, 0);
        // Player events before a pick change nothing and report nothing.
        f.resolved(0);
        assert_eq!(f.started(), None);
        f.stalled(false);
        assert_eq!(f.fail(0, err()), None);
        assert_eq!(f.phase, Phase::Manual);
        assert_eq!(f.to_resolve(), None);
        // Picking starts exactly one resolution.
        assert!(f.pick("anipro|web|Beta"));
        assert_eq!(f.to_resolve(), Some(2));
        assert_eq!(f.attempt, 1);
        assert!(f.sources[2].picked);
        assert!(!f.pick("nope|web|x"));
        assert_eq!(f.attempt, 1);
    }

    #[test]
    fn manual_mode_with_a_saved_pick_uses_it_directly() {
        let mut v = list();
        v[4].picked = true;
        let f = FlowState::new(v, SourceModeVm::Manual, &any());
        assert_eq!(f.to_resolve(), Some(4));
    }

    #[test]
    fn auto_mode_prefers_the_pick_then_the_first_playable() {
        let f = FlowState::new(list(), SourceModeVm::Auto, &any());
        assert_eq!(f.to_resolve(), Some(0));
        let mut v = list();
        v[2].picked = true;
        assert_eq!(FlowState::new(v, SourceModeVm::Auto, &any()).to_resolve(), Some(2));
        // A disabled extension at the top is skipped; a torrent is played like any other source:
        // [Gamma(off), SubsPlease, Beta, Ok.ru].
        let mut v = list();
        v.remove(0);
        v.swap(0, 2);
        v.swap(1, 2);
        assert_eq!((v[0].ext_id.as_str(), v[1].kind), ("off", SourceKindVm::Torrent));
        assert_eq!(FlowState::new(v, SourceModeVm::Auto, &any()).to_resolve(), Some(1));
    }

    #[test]
    fn failover_follows_the_list_and_lists_every_failure() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto, &any());
        let a = f.attempt;
        assert_eq!(f.fail(a, err()).as_deref(), Some("anipro|web|Alpha"));
        // The torrent is next in ranking order (wave 3: torrents play).
        assert_eq!(f.to_resolve(), Some(1));
        let a = f.attempt;
        assert_eq!(f.fail(a, UiError::new(ErrorKind::NoSource, "no peers")).as_deref(), Some("nyaa|torrent|SubsPlease"));
        // Then Beta; the disabled extension is skipped later on.
        assert_eq!(f.to_resolve(), Some(2));
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.phase, Phase::Buffering);
        assert_eq!(f.fail(a, UiError::new(ErrorKind::Other, "mpv: 404")).as_deref(), Some("anipro|web|Beta"));
        assert_eq!(f.to_resolve(), Some(4));
        let a = f.attempt;
        assert_eq!(f.fail(a, UiError::new(ErrorKind::Offline, "")).as_deref(), Some("toko|web|Ok.ru"));
        assert_eq!(f.phase, Phase::Error);
        assert_eq!(f.to_resolve(), None);
        let names: Vec<_> = f.failed.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["anipro · Alpha", "nyaa · SubsPlease", "anipro · Beta", "toko · Ok.ru"]);
        assert_eq!(f.failed[0].detail, "HTTP 403");
        assert_eq!(f.error.as_ref().map(|e| e.kind), Some(ErrorKind::Offline));
        // Retry starts over from the top.
        f.retry();
        assert!(f.failed.is_empty());
        assert_eq!(f.to_resolve(), Some(0));
    }

    #[test]
    fn stale_attempts_are_ignored() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto, &any());
        let old = f.attempt;
        f.pick("anipro|web|Beta");
        assert_eq!(f.fail(old, err()), None);
        f.resolved(old);
        assert_eq!(f.phase, Phase::Resolving);
        assert!(f.failed.is_empty());
    }

    #[test]
    fn manual_mode_stops_on_a_failed_torrent_and_offers_the_best_web_source() {
        let mut f = FlowState::new(list(), SourceModeVm::Manual, &any());
        f.pick("nyaa|torrent|SubsPlease");
        assert_eq!(f.to_resolve(), Some(1), "a picked torrent is resolved, not rejected");
        let a = f.attempt;
        assert!(f.fail(a, UiError::new(ErrorKind::NoSource, "0123456789abcdef0123456789abcdef01234567")).is_some());
        assert_eq!(f.phase, Phase::Error);
        assert_eq!(f.error.as_ref().map(|e| e.kind), Some(ErrorKind::NoSource));
        assert!(f.last_failed_torrent());
        assert_eq!(f.best_web().map(|s| s.label.as_str()), Some("Alpha"));
        // best_web never offers a torrent.
        let only_torrents = FlowState::new(vec![src("nyaa", "Erai", SourceKindVm::Torrent, "1080p")], SourceModeVm::Manual, &any());
        assert!(only_torrents.best_web().is_none());
    }

    #[test]
    fn started_reports_once_and_stalls_toggle_buffering() {
        let mut f = FlowState::new(list(), SourceModeVm::Auto, &any());
        assert_eq!(f.started(), None, "not loaded yet");
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.started().as_deref(), Some("anipro|web|Alpha"));
        assert_eq!(f.phase, Phase::Playing);
        f.stalled(true);
        assert_eq!(f.phase, Phase::Buffering);
        assert_eq!(f.started(), None);
        f.stalled(false);
        assert_eq!(f.phase, Phase::Playing);
    }

    #[test]
    fn empty_or_unplayable_lists_end_in_an_error() {
        let f = FlowState::new(Vec::new(), SourceModeVm::Auto, &any());
        assert_eq!((f.phase, f.error.map(|e| e.kind)), (Phase::Error, Some(ErrorKind::NoSource)));
        // A torrent-only list plays the torrent (no more M2 rejection).
        let f = FlowState::new(vec![src("nyaa", "Erai", SourceKindVm::Torrent, "1080p")], SourceModeVm::Auto, &any());
        assert_eq!((f.phase, f.to_resolve()), (Phase::Resolving, Some(0)));
        let mut off = src("x", "y", SourceKindVm::Web, "");
        off.ext_enabled = false;
        let f = FlowState::new(vec![off], SourceModeVm::Auto, &any());
        assert_eq!(f.error.map(|e| e.kind), Some(ErrorKind::NoSource));
    }

    #[test]
    fn legacy_flow_never_reports_and_never_waits() {
        let v = vec![src("p", "1", SourceKindVm::Web, ""), src("p", "2", SourceKindVm::Web, "")];
        let mut f = FlowState::legacy(v);
        assert_eq!(f.to_resolve(), Some(0));
        let a = f.attempt;
        assert_eq!(f.fail(a, err()), None);
        assert_eq!(f.to_resolve(), Some(1));
        let a = f.attempt;
        f.resolved(a);
        assert_eq!(f.started(), None);
        assert_eq!(f.phase, Phase::Playing);
    }

    #[test]
    fn quality_menu_lists_playable_qualities_highest_first() {
        let f = FlowState::new(list(), SourceModeVm::Auto, &any());
        assert_eq!(f.qualities(), ["1080p", "720p", "480p"]);
        assert_eq!(f.source_for_quality("720p").map(|s| s.label.as_str()), Some("Beta"));
        assert!(f.source_for_quality("2160p").is_none());
        assert_eq!(quality_rank("1080p"), 1080);
        assert_eq!(quality_rank(""), 0);
    }

    #[test]
    fn markers_skip_and_bar_spans() {
        let m = Markers {
            intro: Some(Span { start: 85.0, end: 175.0 }),
            outro: Some(Span { start: 1300.0, end: 1400.0 }),
            recap: None,
        };
        assert_eq!(m.skip_target(90.0), Some(175.0));
        assert_eq!(m.skip_target(175.0), None);
        assert_eq!(m.skip_target(10.0), None);
        let spans = m.bar_spans(1400.0);
        assert_eq!(spans.len(), 2);
        assert!((spans[0].0 - 85.0 / 14.0).abs() < 1e-9);
        assert!(Markers::default().bar_spans(1400.0).is_empty());
        assert!(m.bar_spans(0.0).is_empty());
        assert_eq!(m.skip_at(1350.0), Some((SkipKindVm::Outro, 1400.0)));
        assert_eq!(m.bar_marks(1400.0)[1].kind, SkipKindVm::Outro);
    }

    #[test]
    fn markers_from_skip_marks_keep_the_earliest_of_each_kind() {
        let mk = |kind, a, b| SkipMarkVm { kind, start_secs: a, end_secs: b };
        let m = Markers::from_skips(&[
            mk(SkipKindVm::Intro, 300.0, 390.0),
            mk(SkipKindVm::Intro, 80.0, 170.0),
            mk(SkipKindVm::Recap, 0.0, 40.0),
            mk(SkipKindVm::Outro, 50.0, 10.0),
        ]);
        assert_eq!(m.intro, Some(Span { start: 80.0, end: 170.0 }));
        assert_eq!(m.recap, Some(Span { start: 0.0, end: 40.0 }));
        assert_eq!(m.outro, None, "inverted intervals are dropped");
        assert_eq!(m.skip_at(10.0), Some((SkipKindVm::Recap, 40.0)));
        assert_eq!(m.skip_at(100.0), Some((SkipKindVm::Intro, 170.0)));
        assert_eq!(m.skip_at(200.0), None);
        let kinds: Vec<_> = m.bar_marks(1440.0).into_iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [SkipKindVm::Recap, SkipKindVm::Intro]);
    }

    #[test]
    fn torrent_ranges_on_the_seek_bar() {
        let ranges = [(0.0, 0.25), (0.5, 0.75)];
        assert_eq!(range_spans(&ranges), vec![(0.0, 25.0), (50.0, 25.0)]);
        assert!(range_spans(&[(0.4, 0.4), (f64::NAN, 1.0)]).is_empty());
        assert!((downloaded_fraction(&ranges) - 0.5).abs() < 1e-9);
        assert_eq!(downloaded_fraction(&[]), 0.0);
        assert_eq!(downloaded_fraction(&[(0.0, 1.0)]), 1.0);
        // Ready from the playhead to the end of its range; nothing when the playhead is not downloaded.
        let r = ready_span(&ranges, 0.1).expect("inside the first range");
        assert!((r.0 - 10.0).abs() < 1e-9 && (r.1 - 15.0).abs() < 1e-9);
        assert_eq!(ready_span(&ranges, 0.3), None);
        assert_eq!(ready_span(&ranges, 0.75), None);
        assert!(ready_span(&ranges, 0.5).is_some());
    }

    #[test]
    fn rates_bytes_and_hashes() {
        assert_eq!(format_bytes(12), "12 B");
        assert_eq!(format_bytes(512_000), "512 KB");
        assert_eq!(format_bytes(4_200_000), "4.20 MB");
        assert_eq!(format_bytes(13_800_000_000), "13.8 GB");
        assert_eq!(format_rate(4_200_000), "4.20 MB/s");
        assert_eq!(format_rate(0), "0 B/s");
        assert!(is_info_hash("0123456789abcdef0123456789ABCDEF01234567"));
        assert!(!is_info_hash("toko|torrent|SubsPlease"));
        assert!(!is_info_hash("0123"));
    }

    #[test]
    fn helpers() {
        assert_eq!(hover_fraction(50.0, 200.0), 0.25);
        assert_eq!(hover_fraction(-5.0, 200.0), 0.0);
        assert_eq!(hover_fraction(500.0, 200.0), 1.0);
        assert_eq!(hover_fraction(5.0, 0.0), 0.0);
        assert!(keeps_controls_up(Phase::Resolving, false));
        assert!(keeps_controls_up(Phase::Manual, false));
        assert!(keeps_controls_up(Phase::Buffering, false));
        assert!(!keeps_controls_up(Phase::Buffering, true), "a stall mid-episode does not pin the controls");
        assert!(!keeps_controls_up(Phase::Playing, true));
        assert!(!keeps_controls_up(Phase::Error, false));
        assert_eq!(countdown_left(1000, 1000, 10), 10);
        assert_eq!(countdown_left(1000, 4500, 10), 7);
        assert_eq!(countdown_left(1000, 60_000, 10), 0);
        assert_eq!(countdown_left(1000, 3000, 5), 3);
        assert_eq!(format_speed(1.0), "1×");
        assert_eq!(format_speed(1.25), "1.25×");
        assert_eq!(format_speed(1.5), "1.5×");
        assert_eq!(format_speed(2.0), "2×");
    }

    #[test]
    fn web_keys_map_to_mpv_names() {
        assert_eq!(mpv_key_name(" ", false, false, false, false).as_deref(), Some("SPACE"));
        assert_eq!(mpv_key_name("ArrowLeft", false, false, true, false).as_deref(), Some("Shift+LEFT"));
        assert_eq!(mpv_key_name("S", false, false, true, false).as_deref(), Some("S"));
        assert_eq!(mpv_key_name("s", true, false, false, false).as_deref(), Some("Ctrl+s"));
        assert_eq!(mpv_key_name("#", false, false, true, false).as_deref(), Some("SHARP"));
        assert_eq!(mpv_key_name("F5", false, false, false, false).as_deref(), Some("F5"));
        assert_eq!(mpv_key_name("Shift", false, false, true, false), None);
        assert_eq!(mpv_key_name("Unidentified", false, false, false, false), None);
        assert_eq!(mpv_mouse_button(2), Some("MBTN_RIGHT"));
        assert_eq!(mpv_mouse_button(9), None);
    }
    #[test]
    fn a_remembered_pick_that_no_longer_meets_the_prefs_is_ignored() {
        // Audit bug: JA audio + ES subtitles preferred; a Russian torrent without subtitles remembered from
        // another session must not be preselected.
        let mut v = list();
        let mut ru = src("nyaa", "RuDub", SourceKindVm::Torrent, "1080p");
        ru.subs = Vec::new();
        ru.dubs = vec!["ru".into()];
        ru.picked = true;
        v.push(ru);
        let prefs = SourcePrefsVm::default();
        let f = FlowState::new(v, SourceModeVm::Auto, &prefs);
        assert_eq!(f.dropped_pick.as_deref(), Some("nyaa|torrent|RuDub"));
        assert!(!f.has_pick());
        assert_eq!(f.to_resolve(), Some(0), "the recommended source plays");
        assert!(f.sources.iter().any(|s| s.id == "nyaa|torrent|RuDub"), "the old pick stays as an option");
        // In Manual mode the user is asked again instead of replaying the stale pick.
        let mut v = list();
        v[4].picked = true; // 480p, below the default 720p minimum
        let f = FlowState::new(v, SourceModeVm::Manual, &prefs);
        assert_eq!(f.dropped_pick.as_deref(), Some("toko|web|Ok.ru"));
        assert_eq!(f.phase, Phase::Manual);
    }

    #[test]
    fn a_pick_that_meets_the_prefs_or_has_no_better_alternative_stays() {
        let prefs = SourcePrefsVm::default();
        let mut v = list();
        v[2].picked = true; // Beta 720p, ES subs, JA audio
        let f = FlowState::new(v, SourceModeVm::Auto, &prefs);
        assert_eq!((f.dropped_pick.clone(), f.to_resolve()), (None, Some(2)));
        assert!(f.has_pick());
        // Nothing meets the prefs: the user's choice is kept.
        let mut only = src("nyaa", "RuDub", SourceKindVm::Torrent, "1080p");
        only.subs = Vec::new();
        only.dubs = vec!["ru".into()];
        only.picked = true;
        let mut other = src("nyaa", "EnDub", SourceKindVm::Torrent, "1080p");
        other.subs = Vec::new();
        other.dubs = vec!["en".into()];
        let f = FlowState::new(vec![other, only], SourceModeVm::Auto, &prefs);
        assert_eq!((f.dropped_pick.clone(), f.to_resolve()), (None, Some(1)));
    }

    #[test]
    fn prefs_match_languages_quality_and_dubs_in_the_reading_language() {
        let prefs = SourcePrefsVm::default(); // es subs, ja audio, 720p
        let mut s = src("a", "b", SourceKindVm::Web, "1080p");
        assert!(meets_prefs(&s, &prefs));
        s.subs = vec!["es-419".into()];
        assert!(meets_prefs(&s, &prefs), "regional variants count");
        s.subs = vec!["en".into()];
        assert!(!meets_prefs(&s, &prefs));
        s.dubs = vec!["es".into()];
        assert!(meets_prefs(&s, &prefs), "dubbed in the subtitle language needs no subtitles");
        s.dubs = Vec::new();
        s.subs = vec!["ES".into()];
        assert!(meets_prefs(&s, &prefs), "unknown audio with the right subtitles");
        s.quality = "480p".into();
        assert!(!meets_prefs(&s, &prefs));
        s.quality = String::new();
        assert!(meets_prefs(&s, &prefs), "unknown quality is not held against a source");
        s.subs = vec!["esperanto".into()];
        assert!(!meets_prefs(&s, &prefs), "a prefix that is not a region does not count");
    }

    #[test]
    fn auto_skip_jumps_once_to_the_end_of_the_intro() {
        let m = Markers { intro: Some(Span { start: 85.0, end: 175.0 }), outro: None, recap: None };
        assert_eq!(auto_skip_target(&m, 90.0, false), Some(175.0));
        assert_eq!(auto_skip_target(&m, 90.0, true), None);
        assert_eq!(auto_skip_target(&m, 20.0, false), None);
        assert_eq!(auto_skip_target(&Markers::default(), 90.0, false), None);
    }

    #[test]
    fn speed_volume_and_track_steps() {
        assert_eq!(speed_step(1.0, true), 1.25);
        assert_eq!(speed_step(1.0, false), 0.75);
        assert_eq!(speed_step(2.0, true), 2.0);
        assert_eq!(speed_step(0.5, false), 0.5);
        assert_eq!(speed_step(1.1, true), 1.25);
        assert_eq!(volume_step(98, true), 100);
        assert_eq!(volume_step(3, false), 0);
        assert_eq!(volume_step(50, true), 55);
        let tr = |id, kind| TrackInfo { id, kind, lang: None, title: None, selected: false };
        let tracks = [tr(1, TrackKind::Audio), tr(2, TrackKind::Audio), tr(1, TrackKind::Sub), tr(3, TrackKind::Sub)];
        assert_eq!(cycle_track(&tracks, TrackKind::Sub, 0, true), Some(1));
        assert_eq!(cycle_track(&tracks, TrackKind::Sub, 1, true), Some(3));
        assert_eq!(cycle_track(&tracks, TrackKind::Sub, 3, true), Some(0), "off after the last subtitle");
        assert_eq!(cycle_track(&tracks, TrackKind::Audio, 2, false), Some(1));
        assert_eq!(cycle_track(&tracks, TrackKind::Audio, 9, false), Some(1));
        assert_eq!(cycle_track(&[], TrackKind::Sub, 0, true), None);
    }

    #[test]
    fn sleep_timer_runs_out_and_reports_its_state() {
        let t = SleepTimer::from_choice(SleepChoice::Minutes(15), 1_000);
        assert_eq!(t.attr(), "15");
        assert!(!t.due(1_000 + 14 * 60_000));
        assert!(t.due(1_000 + 15 * 60_000));
        assert_eq!(t.left_secs(1_000 + 14 * 60_000 + 500), Some(60));
        assert_eq!(t.choice(), SleepChoice::Minutes(15));
        let e = SleepTimer::from_choice(SleepChoice::EndOfEpisode, 0);
        assert_eq!((e.attr(), e.due(u64::MAX), e.left_secs(0)), ("end".to_owned(), false, None));
        assert!(e.is_on());
        let off = SleepTimer::from_choice(SleepChoice::Off, 0);
        assert_eq!((off.attr(), off.is_on()), ("off".to_owned(), false));
        assert_eq!(SleepTimer::from_choice(SleepChoice::Minutes(0), 0), SleepTimer::Off);
    }
}
