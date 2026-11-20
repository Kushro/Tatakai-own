//! Data of the Details header compositions (`Settings::look.header_layout`, `data-dhead`): the next-episode card
//! (episode, title, resume progress), the labelled facts and the watched count. Pure; private to
//! `screens/details.rs`.

use crate::backend::{AiringVm, DetailsVm, EpisodeVm};
use crate::i18n::{t, tf, tn};
use crate::model::{HeaderLayout, format_time};

use super::extra::airing_when;
use super::{WATCHED_AT, WatchAction, score_label, status_label};

/// Where the episode to continue stands: the saved position and, when known, the episode length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumeProgress {
    pub watched_secs: u32,
    /// Always longer than `watched_secs` when known.
    pub duration_secs: Option<u32>,
}

impl ResumeProgress {
    /// Watched fraction, 0..1, when the length is known.
    pub fn fraction(&self) -> Option<f64> {
        self.duration_secs.map(|d| (f64::from(self.watched_secs) / f64::from(d.max(1))).clamp(0.0, 1.0))
    }

    pub fn remaining_secs(&self) -> Option<u32> {
        self.duration_secs.map(|d| d.saturating_sub(self.watched_secs))
    }
}

/// Within this share of the listed length, the length derived from the history is the same episode (rounding of the
/// saved fraction): the listed one wins ("quedan 20:21", not "20:02").
const SAME_LENGTH: f64 = 0.05;
/// Beyond this share the derived length is another cut (a recap, a stale history row): it is discarded.
const OTHER_LENGTH: f64 = 0.30;

/// The resume point of the episode to continue. Only `Continue` with a saved position has one (a rewatch starts from
/// the top). The length: the listed one (the episode's runtime, else the series' episode length, `fallback_min`)
/// unless the history fraction (`history_watched = progress / duration`; `resume_secs` is that same progress) says
/// otherwise by 5-30 %; the derived one alone when nothing is listed. A length that is not longer than the watched
/// time is discarded.
pub fn resume_progress(
    watch: WatchAction,
    resume_secs: u32,
    ep: Option<&EpisodeVm>,
    fallback_min: Option<u32>,
) -> Option<ResumeProgress> {
    if !matches!(watch, WatchAction::Continue(_)) || resume_secs == 0 {
        return None;
    }
    let from_fraction = ep
        .and_then(|e| e.history_watched)
        .filter(|f| *f > 0.005 && *f < 1.0)
        .map(|f| (f64::from(resume_secs) / f).round() as u32);
    let listed = [ep.and_then(|e| e.runtime_min), fallback_min]
        .into_iter()
        .flatten()
        .map(|m| m * 60)
        .find(|d| *d > resume_secs);
    let derived = from_fraction.filter(|d| *d > resume_secs);
    let duration_secs = match (derived, listed) {
        (Some(d), Some(l)) => {
            let off = (f64::from(d) - f64::from(l)).abs() / f64::from(l);
            if (SAME_LENGTH..=OTHER_LENGTH).contains(&off) { Some(d) } else { Some(l) }
        }
        (d, l) => d.or(l),
    };
    Some(ResumeProgress { watched_secs: resume_secs, duration_secs })
}

/// Episodes the list marks as watched (the rows' rule, not the history), up to the total when it is known.
pub fn watched_count(eps: &[EpisodeVm], total: Option<u32>) -> u32 {
    let in_range = |n: u32| total.filter(|t| *t > 0).is_none_or(|t| n <= t);
    eps.iter().filter(|e| e.watched >= WATCHED_AT && in_range(e.number)).count() as u32
}

/// What the next-episode card shows.
#[derive(Debug, Clone, PartialEq)]
pub struct NextUp {
    pub action: WatchAction,
    /// The episode the card talks about: the one to continue (up to date: the unaired one).
    pub episode: u32,
    pub total: Option<u32>,
    /// `None` while the episodes load or when the providers have no title.
    pub title: Option<String>,
    pub progress: Option<ResumeProgress>,
}

impl Default for NextUp {
    fn default() -> Self {
        Self { action: WatchAction::Start, episode: 1, total: None, title: None, progress: None }
    }
}

impl NextUp {
    /// A single-episode title (a film): the card names its format instead of "Ep. 1 of 1".
    pub fn single(&self) -> bool {
        self.total == Some(1)
    }

    /// The card's first line: "Ep. 4 of 12", "You watched every episode", "Ep. 9 in 2 d 5 h", "Coming soon · in 3 d".
    pub fn overline(&self, next: Option<AiringVm>, now: i64, format: Option<&str>) -> String {
        let n = self.episode.to_string();
        let countdown = |ep: Option<u32>| {
            next.filter(|a| ep.is_none_or(|e| a.episode == e)).map(|a| airing_when(a.at_unix, now))
        };
        match self.action {
            WatchAction::Rewatch if self.single() => t("details.next.watched_one"),
            WatchAction::Rewatch => t("details.next.all_watched"),
            WatchAction::UpToDate(ep) => match countdown(Some(ep)) {
                Some(Some(when)) => tf("details.next_airing_short", &[("n", &n), ("when", &when)]),
                Some(None) => tf("details.next_airing_short.now", &[("n", &n)]),
                None => tf("details.ep_short", &[("n", &n)]),
            },
            WatchAction::Upcoming => match countdown(None).flatten() {
                Some(when) => format!("{} · {}", t("details.upcoming"), tf("details.fact.next_in", &[("when", &when)])),
                None => t("details.upcoming"),
            },
            WatchAction::Start | WatchAction::Continue(_) => {
                if self.single() {
                    let key = format.map(|f| format!("details.format.{}", f.to_ascii_lowercase()));
                    return match key.map(|k| (t(&k), k)) {
                        Some((label, k)) if label != k => label,
                        _ => tf("details.ep_short", &[("n", &n)]),
                    };
                }
                match self.total.filter(|t| *t > 1) {
                    Some(total) => tf("details.next.ep_of", &[("n", &n), ("total", &total.to_string())]),
                    None => tf("details.ep_short", &[("n", &n)]),
                }
            }
        }
    }
}

/// The card's data from the main action, the episode list (`None` while it loads) and the resume point.
pub fn next_up(
    watch: WatchAction,
    total: Option<u32>,
    eps: Option<&[EpisodeVm]>,
    resume_secs: u32,
    fallback_min: Option<u32>,
) -> NextUp {
    let episode = match watch {
        WatchAction::UpToDate(n) => n.max(1),
        other => other.episode(),
    };
    let ep = eps.and_then(|list| list.iter().find(|e| e.number == episode));
    NextUp {
        action: watch,
        episode,
        total,
        title: ep.and_then(|e| e.title.clone()).filter(|t| !t.trim().is_empty() && !just_the_number(t, episode)),
        progress: resume_progress(watch, resume_secs, ep, fallback_min),
    }
}

/// A title that only repeats the number ("Episode 4", "Episodio 4", "Ep. 04", "4"): providers (`ext:`) often have
/// no real title.
fn just_the_number(title: &str, n: u32) -> bool {
    let lower = title.trim().to_lowercase();
    let digits: String = lower.chars().filter(char::is_ascii_digit).collect();
    let words: String = lower.chars().filter(|c| c.is_alphabetic()).collect();
    digits.parse::<u32>().ok() == Some(n)
        && ["", "episode", "episodio", "ep", "capitulo", "capítulo", "chapter", "cap"].contains(&words.as_str())
}

/// The resume note under the main action of A / B: "3:39 of 24 min", or "Resume at 3:39" without a length.
pub fn progress_note(p: &ResumeProgress) -> String {
    let at = format_time(f64::from(p.watched_secs));
    match p.duration_secs {
        Some(d) => tf("details.progress_of", &[("t", &at), ("n", &((d + 30) / 60).to_string())]),
        None => tf("details.resume_at", &[("t", &at)]),
    }
}

/// The extension a provider title (`ext:`) comes from (`MediaVm::subtitle`), shown with the facts.
pub fn provider_source(vm: &DetailsVm) -> Option<String> {
    let name = vm.media.subtitle.trim();
    (vm.media.id.starts_with("ext:") && !name.is_empty()).then(|| name.to_owned())
}

/// What the header shows around the source and the resume point, by composition (pure; `DetailsView` and
/// `DetailsHeader` follow it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderPlan {
    /// The editorial source line starts with its episode ("Ep. 4 · …"): B, and "up to date" (it follows the last
    /// aired episode, not the one the header talks about).
    pub line_ep: bool,
    /// "Change (n)" instead of "Change source (n)": all but A.
    pub line_short: bool,
    /// The resume point as the main button's second line ("3:39 of 24 min"): A and B.
    pub main_note: bool,
    /// "Resume at 3:39" as its own note beside the actions: the embedded pane only.
    pub resume_note: bool,
}

pub fn header_plan(layout: HeaderLayout, watch: WatchAction, resume_secs: u32, embedded: bool) -> HeaderPlan {
    let resuming = matches!(watch, WatchAction::Continue(_)) && resume_secs > 0;
    HeaderPlan {
        line_ep: layout == HeaderLayout::Facts || matches!(watch, WatchAction::UpToDate(_)),
        line_short: layout != HeaderLayout::Line,
        main_note: !embedded && resuming && matches!(layout, HeaderLayout::Line | HeaderLayout::Facts),
        resume_note: embedded && resuming,
    }
}

/// One labelled fact of B / D (`dl.tk-dhead__facts`): "Episodes" → "3" + "of 12 watched".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadFact {
    /// `status | source | next | episodes | year | score` (`data-fact`).
    pub id: &'static str,
    pub label: String,
    pub value: String,
    pub note: Option<String>,
}

/// The labelled facts, each left out when its data is missing (no filler dashes). `watched`: the list's watched count
/// (`None` while the episodes load).
pub fn head_facts(vm: &DetailsVm, next: Option<AiringVm>, now: i64, watched: Option<u32>) -> Vec<HeadFact> {
    let mut out = Vec::new();
    let fact = |id, label: String, value: String, note: Option<String>| HeadFact { id, label, value, note };
    if let Some(s) = vm.status.as_deref() {
        out.push(fact("status", t("details.status"), status_label(s), None));
    }
    if let Some(src) = provider_source(vm) {
        out.push(fact("source", t("details.fact.source"), src, None));
    }
    if let Some(a) = next {
        let note = match airing_when(a.at_unix, now) {
            Some(when) => tf("details.fact.next_in", &[("when", &when)]),
            None => t("details.fact.next_now"),
        };
        out.push(fact("next", t("details.fact.next"), tf("details.ep_short", &[("n", &a.episode.to_string())]), Some(note)));
    }
    let seen = watched.unwrap_or(0);
    match vm.episodes_total.filter(|t| *t > 0) {
        Some(1) => {}
        Some(total) if seen > 0 => out.push(fact(
            "episodes",
            t("details.episodes"),
            seen.to_string(),
            Some(tf("details.fact.watched_of", &[("total", &total.to_string())])),
        )),
        Some(total) => out.push(fact("episodes", t("details.episodes"), total.to_string(), None)),
        None if seen > 0 => out.push(fact(
            "episodes",
            t("details.episodes"),
            seen.to_string(),
            Some(tn("details.fact.watched", seen as usize)),
        )),
        None => {}
    }
    if let Some(y) = vm.media.year {
        out.push(fact("year", t("details.fact.year"), y.to_string(), None));
    }
    if let Some(s) = vm.media.score {
        out.push(fact("score", t("details.fact.score"), score_label(s), None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaVm;

    fn ep(n: u32, watched: f64) -> EpisodeVm {
        EpisodeVm { number: n, watched, ..EpisodeVm::default() }
    }

    fn vm(status: Option<&str>, total: Option<u32>, year: Option<i32>, score: Option<u32>) -> DetailsVm {
        DetailsVm {
            media: MediaVm { year, score, ..MediaVm::default() },
            banner: None,
            fanart: None,
            synopsis: None,
            genres: Vec::new(),
            status: status.map(str::to_owned),
            episodes_total: total,
            resume_episode: 0,
            resume_secs: 0,
        }
    }

    #[test]
    fn resume_progress_takes_the_length_from_the_history_then_the_runtimes() {
        let c = WatchAction::Continue(4);
        // Nothing listed: the length from the history fraction.
        let e = EpisodeVm { history_watched: Some(0.152), ..ep(4, 0.152) };
        let p = resume_progress(c, 219, Some(&e), None).unwrap();
        assert!(p.duration_secs.unwrap().abs_diff(1440) <= 2, "{p:?}");
        // Within 5 % of the listed length (23:41 derived vs 24 min): the listed one, so 20:21 are left, not 20:02.
        let e = EpisodeVm { history_watched: Some(219.0 / 1421.0), runtime_min: Some(24), ..ep(4, 0.15) };
        let p = resume_progress(c, 219, Some(&e), None).unwrap();
        assert_eq!((p.duration_secs, p.remaining_secs()), (Some(1440), Some(1221)));
        // 5-30 % off: the derived one (a longer cut); more than 30 %: discarded.
        let e = EpisodeVm { history_watched: Some(219.0 / 1680.0), runtime_min: Some(24), ..ep(4, 0.13) };
        assert_eq!(resume_progress(c, 219, Some(&e), None).unwrap().duration_secs, Some(1680));
        let e = EpisodeVm { history_watched: Some(219.0 / 3000.0), runtime_min: Some(24), ..ep(4, 0.07) };
        assert_eq!(resume_progress(c, 219, Some(&e), None).unwrap().duration_secs, Some(1440));
        let e = EpisodeVm { history_watched: Some(0.152), runtime_min: Some(23), ..ep(4, 0.152) };
        let e = EpisodeVm { history_watched: None, ..e };
        assert_eq!(resume_progress(c, 219, Some(&e), Some(25)).unwrap().duration_secs, Some(23 * 60));
        assert_eq!(resume_progress(c, 219, None, Some(25)).unwrap().duration_secs, Some(25 * 60));
        assert_eq!(resume_progress(c, 219, None, None).unwrap().duration_secs, None);
        // A fraction of 0 or of the whole episode says nothing about the length.
        for f in [0.0, 1.0, 1.2] {
            let e = EpisodeVm { history_watched: Some(f), ..ep(4, 0.0) };
            assert_eq!(resume_progress(c, 219, Some(&e), None).unwrap().duration_secs, None, "{f}");
        }
        // A length not longer than the watched time is discarded.
        let e = EpisodeVm { runtime_min: Some(3), ..ep(4, 0.0) };
        assert_eq!(resume_progress(c, 219, Some(&e), Some(24)).unwrap().duration_secs, Some(24 * 60));
        assert_eq!(resume_progress(c, 1500, Some(&e), Some(24)).unwrap().duration_secs, None);
        for w in [WatchAction::Start, WatchAction::Rewatch, WatchAction::UpToDate(4), WatchAction::Upcoming] {
            assert_eq!(resume_progress(w, 219, None, Some(24)), None, "{w:?}");
        }
        assert_eq!(resume_progress(c, 0, None, Some(24)), None);
    }

    #[test]
    fn remaining_time_and_fraction() {
        let p = ResumeProgress { watched_secs: 219, duration_secs: Some(1440) };
        assert_eq!(p.remaining_secs(), Some(1221));
        assert_eq!(format_time(1221.0), "20:21");
        assert!((p.fraction().unwrap() - 0.152).abs() < 0.001);
        assert_eq!(ResumeProgress { watched_secs: 5, duration_secs: None }.fraction(), None);
        assert_eq!(progress_note(&p), "3:39 of 24 min");
        // Rounded to the nearest minute (23:41 is 24 min, 23:20 is 23).
        assert_eq!(progress_note(&ResumeProgress { watched_secs: 219, duration_secs: Some(1421) }), "3:39 of 24 min");
        assert_eq!(progress_note(&ResumeProgress { watched_secs: 219, duration_secs: Some(1400) }), "3:39 of 23 min");
        assert_eq!(progress_note(&ResumeProgress { watched_secs: 219, duration_secs: None }), "Resume at 3:39");
    }

    #[test]
    fn watched_count_follows_the_rows_up_to_the_total() {
        let all: Vec<_> = (1..=12).map(|n| ep(n, 1.0)).collect();
        assert_eq!(watched_count(&all, Some(12)), 12);
        let mut extra = all.clone();
        extra.push(ep(13, 1.0));
        assert_eq!(watched_count(&extra, Some(12)), 12);
        assert_eq!(watched_count(&extra, None), 13);
        assert_eq!(watched_count(&[ep(1, 1.0), ep(2, 0.5), ep(3, 0.95)], Some(12)), 2);
        assert_eq!(watched_count(&[], Some(12)), 0);
    }

    #[test]
    fn next_up_for_every_action() {
        let eps = vec![
            EpisodeVm { title: Some("Item Crafting Face-Off".into()), ..ep(4, 0.1) },
            EpisodeVm { title: Some("Pilot".into()), ..ep(1, 1.0) },
        ];
        let n = next_up(WatchAction::Continue(4), Some(12), Some(&eps), 219, Some(24));
        assert_eq!((n.episode, n.title.as_deref()), (4, Some("Item Crafting Face-Off")));
        assert_eq!(n.progress.unwrap().duration_secs, Some(1440));
        assert_eq!(n.overline(None, 0, Some("TV")), "Ep. 4 of 12");
        let n = next_up(WatchAction::Start, Some(12), Some(&eps), 0, None);
        assert_eq!((n.episode, n.progress), (1, None));
        assert_eq!(n.overline(None, 0, None), "Ep. 1 of 12");
        let n = next_up(WatchAction::Rewatch, Some(12), Some(&eps), 300, Some(24));
        assert_eq!((n.episode, n.progress), (1, None));
        assert_eq!(n.overline(None, 0, None), t("details.next.all_watched"));
        // Up to date with episode 9: the card talks about 9 (the sources follow 8).
        let n = next_up(WatchAction::UpToDate(9), Some(12), None, 0, None);
        assert_eq!((n.episode, n.title.clone()), (9, None));
        let air = AiringVm { episode: 9, at_unix: 1_000 + 2 * 86_400 + 5 * 3_600 };
        assert!(n.overline(Some(air), 1_000, None).starts_with("Ep. 9 in "));
        assert_eq!(n.overline(None, 1_000, None), "Ep. 9");
        assert_eq!(n.overline(Some(AiringVm { episode: 9, at_unix: 10 }), 1_000, None), "Ep. 9 airing now");
        let n = next_up(WatchAction::Upcoming, Some(12), None, 0, None);
        assert_eq!(n.overline(None, 0, None), t("details.upcoming"));
        assert!(n.overline(Some(air), 1_000, None).starts_with(&t("details.upcoming")));
        // A film: its format, else "Ep. 1".
        let n = next_up(WatchAction::Start, Some(1), None, 0, None);
        assert_eq!(n.overline(None, 0, Some("MOVIE")), "Movie");
        assert_eq!(n.overline(None, 0, None), "Ep. 1");
        assert_eq!(n.overline(None, 0, Some("WEIRD")), "Ep. 1");
        // A watched film is not "every episode".
        let n = next_up(WatchAction::Rewatch, Some(1), None, 0, None);
        assert_eq!(n.overline(None, 0, Some("MOVIE")), "You've watched it");
        // Provider titles that only repeat the number are left out.
        for title in ["Episode 4", "Episodio 4", "Ep. 04", "4"] {
            let eps = vec![EpisodeVm { title: Some(title.into()), ..ep(4, 0.0) }];
            assert_eq!(next_up(WatchAction::Continue(4), None, Some(&eps), 0, None).title, None, "{title}");
        }
        let eps = vec![EpisodeVm { title: Some("Episode 4: The Duel".into()), ..ep(4, 0.0) }];
        assert!(next_up(WatchAction::Continue(4), None, Some(&eps), 0, None).title.is_some());
        // Unknown total.
        assert_eq!(next_up(WatchAction::Continue(30), None, None, 0, None).overline(None, 0, None), "Ep. 30");
    }

    #[test]
    fn facts_leave_out_what_is_missing() {
        let air = AiringVm { episode: 9, at_unix: 1_000 + 2 * 86_400 + 5 * 3_600 };
        let f = head_facts(&vm(Some("RELEASING"), Some(12), Some(2026), Some(76)), Some(air), 1_000, Some(3));
        let ids: Vec<_> = f.iter().map(|x| x.id).collect();
        assert_eq!(ids, ["status", "next", "episodes", "year", "score"]);
        let eps = &f[2];
        assert_eq!((eps.value.as_str(), eps.note.as_deref()), ("3", Some("of 12 watched")));
        assert_eq!(f[1].value, "Ep. 9");
        assert!(f[1].note.as_deref().unwrap().starts_with("in "));
        assert_eq!(f[4].value, "7.6");
        // Nothing watched (or still loading): the total alone.
        let f = head_facts(&vm(None, Some(12), None, None), None, 0, Some(0));
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].id, f[0].value.as_str(), f[0].note.clone()), ("episodes", "12", None));
        assert_eq!(head_facts(&vm(None, Some(12), None, None), None, 0, None)[0].value, "12");
        // Unknown total with watched episodes; a film has no episodes fact.
        let f = head_facts(&vm(None, None, None, None), None, 0, Some(3));
        assert_eq!((f[0].value.as_str(), f[0].note.as_deref()), ("3", Some("watched")));
        assert!(head_facts(&vm(None, Some(1), None, None), None, 0, Some(1)).is_empty());
        assert!(head_facts(&vm(None, None, None, None), None, 0, None).is_empty());
        // A provider title names its extension.
        let mut p = vm(None, Some(12), None, None);
        p.media.id = "ext:anizone:123".into();
        p.media.subtitle = "AniZone".into();
        let f = head_facts(&p, None, 0, None);
        assert_eq!((f[0].id, f[0].value.as_str()), ("source", "AniZone"));
        assert_eq!(provider_source(&vm(None, None, None, None)), None);
        // Airing now.
        let f = head_facts(&vm(None, None, None, None), Some(AiringVm { episode: 9, at_unix: 0 }), 10, None);
        assert_eq!(f[0].note.as_deref(), Some("airing now"));
    }

    #[test]
    fn the_plan_follows_the_composition() {
        use HeaderLayout::*;
        let c = WatchAction::Continue(4);
        let p = header_plan(Facts, c, 219, false);
        assert!(p.line_ep && p.line_short && p.main_note && !p.resume_note);
        let p = header_plan(Line, c, 219, false);
        assert!(!p.line_ep && !p.line_short && p.main_note);
        for l in [NextUp, FactsNextUp] {
            let p = header_plan(l, c, 219, false);
            assert!(!p.line_ep && p.line_short && !p.main_note && !p.resume_note, "{l:?}");
            // Up to date: the line names the episode it follows.
            assert!(header_plan(l, WatchAction::UpToDate(9), 0, false).line_ep);
        }
        // Without a saved position, or not continuing, there is no note.
        assert!(!header_plan(Line, c, 0, false).main_note);
        assert!(!header_plan(Facts, WatchAction::Rewatch, 300, false).main_note);
        // The embedded pane: only the "Resume at" note.
        let p = header_plan(FactsNextUp, c, 219, true);
        assert!(p.resume_note && !p.main_note);
    }
}
