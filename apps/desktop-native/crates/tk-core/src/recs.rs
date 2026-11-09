//! Local recommendation engine (wave B, worker **b**; docs/redesign/16-parity-wave-b-contract.md section 3.5;
//! checklist `home.recs.for-you`, `home.recs.explanations`, `home.recs.feedback`, `home.recs.atlas`; classic
//! `hooks/useRecommendationEngine.ts`, `atlas/*`). UI-free, pure functions over the local history + list entries:
//!
//! * [`TasteProfile::build`] turns the titles the viewer has seen ([`Seen`]: genres, studios, year, tags and how much
//!   they liked it) into weights normalised to `0..=1` per feature family; `Like` feedback counts as a fully liked
//!   title, so it boosts the features of that title.
//! * [`score`] rates a [`Candidate`] against the profile: weighted average of the genre / tag affinity, the best studio
//!   and era match plus a small quality prior (AniList mean score). It returns the affinity (`0..=1`) and the
//!   [`Reason`]s that contributed most, which the UI renders with i18n (`Reason::token` = `kind:value`).
//! * [`rank`] scores, drops excluded ids (`NotInterested` / recently skipped feedback, already seen titles) and sorts.
//! * [`atlas`] projects titles to 2-D for the Anime Atlas: genre vectors, PCA by power iteration (no ML crate), each
//!   axis normalised to `0..=1`; the cluster label is the title's genre that is most common in the set.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Weight of each feature family in [`score`] (sums to 1 with [`QUALITY_WEIGHT`]).
const GENRE_WEIGHT: f32 = 0.42;
const TAG_WEIGHT: f32 = 0.2;
const STUDIO_WEIGHT: f32 = 0.14;
const ERA_WEIGHT: f32 = 0.1;
const QUALITY_WEIGHT: f32 = 0.14;
/// Tags below this AniList rank (0..=100) are ignored: they barely describe the title.
pub const MIN_TAG_RANK: u8 = 60;
/// Tags considered per title.
const TAGS_PER_TITLE: usize = 8;
/// Reasons returned per recommendation.
pub const MAX_REASONS: usize = 3;
/// A feature counts as a reason only from this profile weight on.
const REASON_MIN_WEIGHT: f32 = 0.35;

/// One title the viewer has seen (history row, list entry or liked recommendation), with its features.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Seen {
    pub anilist_id: u32,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub year: Option<u16>,
    /// `(tag name, AniList rank 0..=100)`.
    pub tags: Vec<(String, u8)>,
    /// How much it counts, `0..=1` (see [`seen_weight`]).
    pub weight: f32,
}

/// A title that could be recommended.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Candidate {
    pub anilist_id: u32,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub year: Option<u16>,
    pub tags: Vec<(String, u8)>,
    /// AniList `averageScore` (0..=100).
    pub mean_score: Option<u32>,
}

/// Weight of a seen title from what the viewer did with it: a list score wins (`score / 100`, but never below 0.2:
/// even a low score says they watched it), then completed (1.0), dropped (0.15), in progress (0.7), history only (0.6).
pub fn seen_weight(list_score: Option<u8>, completed: bool, dropped: bool) -> f32 {
    match list_score {
        Some(s) if s > 0 => (f32::from(s.min(100)) / 100.0).max(0.2),
        _ if dropped => 0.15,
        _ if completed => 1.0,
        _ => 0.7,
    }
}

/// Weighted feature vector of the viewer's taste (weights `0..=1`, highest first).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TasteProfile {
    pub genres: Vec<(String, f32)>,
    pub studios: Vec<(String, f32)>,
    pub eras: Vec<(String, f32)>,
    pub tags: Vec<(String, f32)>,
    pub sample_size: u32,
}

/// Feature family of a [`Reason`]; `id()` is the token prefix and the i18n suffix `recs.reason.<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasonKind {
    Genre,
    Studio,
    Era,
    Tag,
    /// High AniList mean score (the value is the score).
    Score,
}

impl ReasonKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Genre => "genre",
            Self::Studio => "studio",
            Self::Era => "era",
            Self::Tag => "tag",
            Self::Score => "score",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "genre" => Self::Genre,
            "studio" => Self::Studio,
            "era" => Self::Era,
            "tag" => Self::Tag,
            "score" => Self::Score,
            _ => return None,
        })
    }
}

/// Why a title is recommended: the feature and how much it weighed.
#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    pub kind: ReasonKind,
    pub value: String,
    pub weight: f32,
}

impl Reason {
    /// Wire form for the UI (`genre:Action`, `studio:MAPPA`, `era:2010s`, `score:86`).
    pub fn token(&self) -> String {
        format!("{}:{}", self.kind.id(), self.value)
    }

    /// Inverse of [`Reason::token`] (`weight` is not carried).
    pub fn parse_token(token: &str) -> Option<(ReasonKind, String)> {
        let (k, v) = token.split_once(':')?;
        let v = v.trim();
        if v.is_empty() {
            return None;
        }
        Some((ReasonKind::from_id(k.trim())?, v.to_owned()))
    }
}

/// Decade label of a year (`1999 -> "1990s"`); pure, used for the `eras` feature.
pub fn era_of(year: u16) -> String {
    format!("{}s", year / 10 * 10)
}

fn top_tags(tags: &[(String, u8)]) -> impl Iterator<Item = &(String, u8)> {
    tags.iter()
        .filter(|(_, r)| *r >= MIN_TAG_RANK)
        .take(TAGS_PER_TITLE)
}

/// Normalise a weight map by its maximum and sort it (highest first, then by name for stable output).
fn normalised(map: HashMap<String, f32>) -> Vec<(String, f32)> {
    let max = map.values().copied().fold(0.0_f32, f32::max);
    let mut v: Vec<(String, f32)> = if max > 0.0 {
        map.into_iter()
            .map(|(k, w)| (k, (w / max).clamp(0.0, 1.0)))
            .filter(|(_, w)| *w > 0.0)
            .collect()
    } else {
        Vec::new()
    };
    v.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    v
}

impl TasteProfile {
    /// Build the profile; a title seen twice (history + list) counts once with its highest weight.
    pub fn build(seen: &[Seen]) -> Self {
        let mut best: HashMap<u32, &Seen> = HashMap::new();
        for s in seen.iter().filter(|s| s.weight > 0.0) {
            match best.get(&s.anilist_id) {
                Some(prev) if prev.weight >= s.weight => {}
                _ => {
                    best.insert(s.anilist_id, s);
                }
            }
        }
        let (mut genres, mut studios, mut eras, mut tags) =
            (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        for s in best.values() {
            let w = s.weight.clamp(0.0, 1.0);
            for g in &s.genres {
                *genres.entry(g.clone()).or_insert(0.0) += w;
            }
            for st in &s.studios {
                *studios.entry(st.clone()).or_insert(0.0) += w;
            }
            if let Some(y) = s.year {
                *eras.entry(era_of(y)).or_insert(0.0) += w;
            }
            for (t, r) in top_tags(&s.tags) {
                *tags.entry(t.clone()).or_insert(0.0) += w * f32::from(*r) / 100.0;
            }
        }
        Self {
            genres: normalised(genres),
            studios: normalised(studios),
            eras: normalised(eras),
            tags: normalised(tags),
            sample_size: u32::try_from(best.len()).unwrap_or(u32::MAX),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sample_size == 0
    }

    fn weight(list: &[(String, f32)], key: &str) -> f32 {
        list.iter().find(|(k, _)| k == key).map_or(0.0, |(_, w)| *w)
    }

    /// The `n` heaviest genres (to fetch candidates).
    pub fn top_genres(&self, n: usize) -> Vec<String> {
        self.genres.iter().take(n).map(|(g, _)| g.clone()).collect()
    }
}

/// Affinity `0..=1` of `c` with the profile and the reasons, strongest first (at most [`MAX_REASONS`]).
pub fn score(p: &TasteProfile, c: &Candidate) -> (f32, Vec<Reason>) {
    let mut reasons: Vec<Reason> = Vec::new();
    let mut consider = |kind: ReasonKind, value: &str, w: f32| {
        if w >= REASON_MIN_WEIGHT {
            reasons.push(Reason {
                kind,
                value: value.to_owned(),
                weight: w,
            });
        }
    };

    let genre = if c.genres.is_empty() {
        0.0
    } else {
        let ws: Vec<f32> = c
            .genres
            .iter()
            .map(|g| TasteProfile::weight(&p.genres, g))
            .collect();
        for (g, w) in c.genres.iter().zip(&ws) {
            consider(ReasonKind::Genre, g, *w);
        }
        // Mean of the two best matches: one strong genre should not be diluted by many minor ones.
        let mut sorted = ws.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let n = sorted.len().min(2);
        sorted.iter().take(n).sum::<f32>() / n as f32
    };
    let tags: Vec<&(String, u8)> = top_tags(&c.tags).collect();
    let tag = if tags.is_empty() {
        0.0
    } else {
        let mut sum = 0.0;
        for (t, _) in &tags {
            let w = TasteProfile::weight(&p.tags, t);
            consider(ReasonKind::Tag, t, w);
            sum += w;
        }
        (sum / tags.len().min(3) as f32).min(1.0)
    };
    let studio = c
        .studios
        .iter()
        .map(|s| (s, TasteProfile::weight(&p.studios, s)))
        .fold(0.0_f32, |acc, (s, w)| {
            consider(ReasonKind::Studio, s, w);
            acc.max(w)
        });
    let era = c.year.map(era_of).map_or(0.0, |e| {
        let w = TasteProfile::weight(&p.eras, &e);
        consider(ReasonKind::Era, &e, w * 0.9);
        w
    });
    let quality = c
        .mean_score
        .map_or(0.5, |s| (s.min(100) as f32 / 100.0).clamp(0.0, 1.0));
    if let Some(s) = c.mean_score.filter(|s| *s >= 80) {
        reasons.push(Reason {
            kind: ReasonKind::Score,
            value: s.to_string(),
            weight: 0.3,
        });
    }

    let total = GENRE_WEIGHT * genre
        + TAG_WEIGHT * tag
        + STUDIO_WEIGHT * studio
        + ERA_WEIGHT * era
        + QUALITY_WEIGHT * quality;
    // Kind order breaks ties so the same input always yields the same explanation.
    reasons.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.value.cmp(&b.value))
    });
    let mut seen_kinds: HashMap<ReasonKind, u8> = HashMap::new();
    reasons.retain(|r| {
        // At most two reasons of one family (two genres read fine, three do not).
        let n = seen_kinds.entry(r.kind).or_insert(0);
        *n += 1;
        *n <= 2
    });
    reasons.truncate(MAX_REASONS);
    (total.clamp(0.0, 1.0), reasons)
}

/// One ranked result of [`rank`]: index into the candidate slice, affinity and reasons.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub score: f32,
    pub reasons: Vec<Reason>,
}

/// Score every candidate not in `excluded`, deduplicated by id, best first.
pub fn rank(p: &TasteProfile, candidates: &[Candidate], excluded: &HashSet<u32>) -> Vec<Ranked> {
    let mut ids = HashSet::new();
    let mut out: Vec<Ranked> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.anilist_id != 0 && !excluded.contains(&c.anilist_id) && ids.insert(c.anilist_id))
        .map(|(index, c)| {
            let (score, reasons) = score(p, c);
            Ranked {
                index,
                score,
                reasons,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });
    out
}

/// Share of recommendations the viewer liked among the ones they judged (`like` vs `not_interested`), once there
/// are at least `min_votes` of them.
pub fn accuracy(likes: u32, dislikes: u32, min_votes: u32) -> Option<f32> {
    let n = likes + dislikes;
    (n >= min_votes.max(1)).then(|| likes as f32 / n as f32)
}

/// One projected point of [`atlas`]: coordinates `0..=1` and the cluster label (a genre, or `""` without genres).
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasPoint {
    pub x: f32,
    pub y: f32,
    pub cluster: String,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalise(v: &mut [f32]) -> bool {
    let n = dot(v, v).sqrt();
    if n <= f32::EPSILON {
        return false;
    }
    for x in v.iter_mut() {
        *x /= n;
    }
    true
}

/// Leading eigenvector of the covariance `cov` (dimension `d`) by power iteration, orthogonal to `against`.
fn principal(cov: &[f32], d: usize, against: Option<&[f32]>) -> Vec<f32> {
    // Deterministic, non-symmetric start so the iteration does not sit on a zero component.
    let mut v: Vec<f32> = (0..d).map(|i| 1.0 + (i as f32 * 0.37).sin()).collect();
    for _ in 0..64 {
        let mut next = vec![0.0_f32; d];
        for (i, n) in next.iter_mut().enumerate() {
            *n = (0..d).map(|j| cov[i * d + j] * v[j]).sum();
        }
        if let Some(a) = against {
            let k = dot(&next, a);
            for (n, x) in next.iter_mut().zip(a) {
                *n -= k * x;
            }
        }
        if !normalise(&mut next) {
            break;
        }
        v = next;
    }
    v
}

/// `(min, max)` of the values.
fn span(vals: impl Iterator<Item = f32>) -> (f32, f32) {
    vals.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    })
}

/// 2-D projection of titles by their genres (see the module doc). Same input, same output.
pub fn atlas(items: &[Vec<String>]) -> Vec<AtlasPoint> {
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for g in items.iter().flatten() {
        *freq.entry(g.as_str()).or_insert(0) += 1;
    }
    let mut vocab: Vec<&str> = freq.keys().copied().collect();
    vocab.sort_by(|a, b| freq[b].cmp(&freq[a]).then_with(|| a.cmp(b)));
    let d = vocab.len();
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }
    let index: HashMap<&str, usize> = vocab.iter().enumerate().map(|(i, g)| (*g, i)).collect();
    let rows: Vec<Vec<f32>> = items
        .iter()
        .map(|gs| {
            let mut r = vec![0.0_f32; d];
            for g in gs {
                if let Some(i) = index.get(g.as_str()) {
                    r[*i] = 1.0;
                }
            }
            r
        })
        .collect();
    let cluster_of = |gs: &Vec<String>| -> String {
        gs.iter()
            .min_by_key(|g| index.get(g.as_str()).copied().unwrap_or(usize::MAX))
            .cloned()
            .unwrap_or_default()
    };
    if d < 2 {
        // Nothing to project: spread the titles on a line, in input order.
        return items
            .iter()
            .enumerate()
            .map(|(i, gs)| AtlasPoint {
                x: if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 },
                y: 0.5,
                cluster: cluster_of(gs),
            })
            .collect();
    }
    let mean: Vec<f32> = (0..d)
        .map(|j| rows.iter().map(|r| r[j]).sum::<f32>() / n as f32)
        .collect();
    let centred: Vec<Vec<f32>> = rows
        .iter()
        .map(|r| r.iter().zip(&mean).map(|(x, m)| x - m).collect())
        .collect();
    let mut cov = vec![0.0_f32; d * d];
    for r in &centred {
        for i in 0..d {
            for j in 0..d {
                cov[i * d + j] += r[i] * r[j];
            }
        }
    }
    let pc1 = principal(&cov, d, None);
    let pc2 = principal(&cov, d, Some(pc1.as_slice()));
    let raw: Vec<(f32, f32)> = centred.iter().map(|r| (dot(r, &pc1), dot(r, &pc2))).collect();
    let (x0, x1) = span(raw.iter().map(|p| p.0));
    let (y0, y1) = span(raw.iter().map(|p| p.1));
    let scale = |v: f32, lo: f32, hi: f32| {
        if hi - lo > f32::EPSILON {
            ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    };
    raw.iter()
        .zip(items)
        .map(|((x, y), gs)| AtlasPoint {
            x: scale(*x, x0, x1),
            y: scale(*y, y0, y1),
            cluster: cluster_of(gs),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(id: u32, genres: &[&str], studio: &str, year: u16, weight: f32) -> Seen {
        Seen {
            anilist_id: id,
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            studios: vec![studio.to_owned()],
            year: Some(year),
            tags: vec![("Shounen".into(), 90), ("Gore".into(), 40)],
            weight,
        }
    }

    fn cand(id: u32, genres: &[&str], studio: &str, year: u16, score: u32) -> Candidate {
        Candidate {
            anilist_id: id,
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            studios: vec![studio.to_owned()],
            year: Some(year),
            tags: vec![("Shounen".into(), 85)],
            mean_score: Some(score),
        }
    }

    #[test]
    fn eras_are_decades() {
        assert_eq!(era_of(1999), "1990s");
        assert_eq!(era_of(2024), "2020s");
    }

    #[test]
    fn seen_weight_prefers_the_list_score() {
        assert_eq!(seen_weight(Some(90), false, false), 0.9);
        assert_eq!(seen_weight(Some(5), true, false), 0.2);
        assert_eq!(seen_weight(None, true, false), 1.0);
        assert_eq!(seen_weight(None, false, true), 0.15);
        assert_eq!(seen_weight(None, false, false), 0.7);
    }

    #[test]
    fn profile_is_normalised_and_dedupes_titles() {
        let p = TasteProfile::build(&[
            seen(1, &["Action", "Drama"], "MAPPA", 2019, 1.0),
            seen(1, &["Action", "Drama"], "MAPPA", 2019, 0.5),
            seen(2, &["Action"], "Bones", 2008, 0.5),
        ]);
        assert_eq!(p.sample_size, 2);
        assert_eq!(p.genres[0], ("Action".to_owned(), 1.0));
        let drama = p.genres.iter().find(|(g, _)| g == "Drama").unwrap().1;
        assert!((drama - 1.0 / 1.5).abs() < 1e-4, "{drama}");
        assert_eq!(p.studios[0].0, "MAPPA");
        assert_eq!(p.eras[0].0, "2010s");
        // low-rank tags are ignored
        assert!(p.tags.iter().all(|(t, _)| t != "Gore"));
        assert_eq!(p.top_genres(1), vec!["Action".to_owned()]);
    }

    #[test]
    fn matching_candidates_score_higher_and_explain_why() {
        let p = TasteProfile::build(&[seen(1, &["Action", "Drama"], "MAPPA", 2019, 1.0)]);
        let (good, reasons) = score(&p, &cand(10, &["Action"], "MAPPA", 2020, 85));
        let (bad, _) = score(&p, &cand(11, &["Romance"], "Kyoto", 1995, 60));
        assert!(good > bad, "{good} <= {bad}");
        assert!((0.0..=1.0).contains(&good));
        assert!(!reasons.is_empty() && reasons.len() <= MAX_REASONS);
        assert_eq!(reasons[0].token(), "genre:Action");
        assert_eq!(
            Reason::parse_token("studio:MAPPA"),
            Some((ReasonKind::Studio, "MAPPA".into()))
        );
        assert_eq!(Reason::parse_token("bogus:x"), None);
        assert_eq!(Reason::parse_token("genre:"), None);
    }

    #[test]
    fn rank_drops_excluded_and_duplicates() {
        let p = TasteProfile::build(&[seen(1, &["Action"], "MAPPA", 2019, 1.0)]);
        let cands = vec![
            cand(5, &["Romance"], "X", 2000, 50),
            cand(6, &["Action"], "MAPPA", 2019, 80),
            cand(6, &["Action"], "MAPPA", 2019, 80),
            cand(7, &["Action"], "Y", 2019, 70),
        ];
        let excluded: HashSet<u32> = [7].into_iter().collect();
        let r = rank(&p, &cands, &excluded);
        let ids: Vec<u32> = r.iter().map(|x| cands[x.index].anilist_id).collect();
        assert_eq!(ids, vec![6, 5]);
    }

    #[test]
    fn accuracy_needs_enough_votes() {
        assert_eq!(accuracy(1, 1, 5), None);
        assert_eq!(accuracy(3, 1, 4), Some(0.75));
    }

    #[test]
    fn atlas_is_normalised_and_clusters_by_common_genre() {
        let items = vec![
            vec!["Action".to_owned(), "Comedy".to_owned()],
            vec!["Action".to_owned()],
            vec!["Romance".to_owned(), "Drama".to_owned()],
            vec!["Romance".to_owned()],
            vec![],
        ];
        let pts = atlas(&items);
        assert_eq!(pts.len(), items.len());
        assert!(
            pts.iter()
                .all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y))
        );
        assert_eq!(pts[1].cluster, "Action");
        assert_eq!(pts[3].cluster, "Romance");
        assert_eq!(pts[4].cluster, "");
        // action titles end up closer to each other than to romance titles
        let d = |a: &AtlasPoint, b: &AtlasPoint| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        assert!(d(&pts[0], &pts[1]) < d(&pts[0], &pts[3]));
        assert_eq!(atlas(&items), pts, "deterministic");
        assert!(atlas(&[]).is_empty());
    }
}
