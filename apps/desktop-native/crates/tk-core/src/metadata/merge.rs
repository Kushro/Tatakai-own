//! 0.3.2: one episode list out of several providers.
//!
//! The first provider with episodes gives the list (its numbering and its own fields win); a later provider fills, by
//! episode number, what the base lacks (title, thumbnail, air date, synopsis, length, rating) and adds episodes the
//! base does not list yet, as long as they already aired. Kitsu lists a new show's episodes as bare numbers while
//! ani.zip (TVDB) has the screencaps and synopses: before this the first non-empty list won and ani.zip was never
//! asked (Overgeared: every card showed the series art and "Episode N").

use std::collections::BTreeMap;

use crate::model::Episode;

/// "Episode 3" / "Ep. 3" / "3" style titles carry no information: treated as missing so a real one can replace them.
fn placeholder_title(t: &str, number: u32) -> bool {
    let t = t.trim().to_ascii_lowercase();
    let n = number.to_string();
    t.is_empty()
        || t == n
        || ["episode", "episodio", "ep.", "ep"].iter().any(|p| t.strip_prefix(p).is_some_and(|rest| rest.trim() == n))
}

/// Whether `e` still misses something another provider could fill.
pub fn lacks_detail(e: &Episode) -> bool {
    e.title.as_deref().is_none_or(|t| placeholder_title(t, e.number))
        || e.image.is_none()
        || e.air_date.is_none()
        || e.summary.is_none()
}

/// Whether any episode of the list is worth asking another provider for.
pub fn needs_fill(eps: &[Episode]) -> bool {
    eps.iter().any(lacks_detail)
}

fn aired_before(e: &Episode, today: &str) -> bool {
    e.air_date.as_deref().is_some_and(|d| d.len() >= 10 && today.len() >= 10 && d[..10] < today[..10])
}

/// `base` with the gaps filled from `extra` (same episode number) plus the episodes of `extra` that `base` does not
/// list and that aired before `today` (`YYYY-MM-DD`). Sorted by number. Pure.
pub fn merge_episodes(base: Vec<Episode>, extra: &[Episode], today: &str) -> Vec<Episode> {
    let mut by_number: BTreeMap<u32, Episode> = BTreeMap::new();
    for e in base {
        by_number.entry(e.number).or_insert(e);
    }
    for x in extra {
        match by_number.get_mut(&x.number) {
            Some(e) => {
                if e.title.as_deref().is_none_or(|t| placeholder_title(t, e.number))
                    && x.title.as_deref().is_some_and(|t| !placeholder_title(t, x.number))
                {
                    e.title = x.title.clone();
                }
                if e.image.is_none() {
                    e.image = x.image.clone();
                }
                if e.air_date.is_none() {
                    e.air_date = x.air_date.clone();
                }
                if e.summary.is_none() {
                    e.summary = x.summary.clone();
                }
                if e.runtime_min.is_none() {
                    e.runtime_min = x.runtime_min;
                }
                if e.rating.is_none() {
                    e.rating = x.rating;
                }
                e.is_filler |= x.is_filler;
            }
            None if aired_before(x, today) => {
                by_number.insert(x.number, x.clone());
            }
            None => {}
        }
    }
    by_number.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(n: u32) -> Episode {
        Episode {
            number: n,
            title: None,
            image: None,
            air_date: None,
            is_filler: false,
            summary: None,
            runtime_min: None,
            rating: None,
        }
    }

    fn rich(n: u32, aired: &str, with_image: bool) -> Episode {
        Episode {
            number: n,
            title: Some(format!("Chapter {n}")),
            image: with_image.then(|| format!("https://img/{n}.jpg")),
            air_date: Some(aired.to_owned()),
            is_filler: false,
            summary: Some(format!("Synopsis {n}")),
            runtime_min: Some(24),
            rating: Some(7.5),
        }
    }

    /// Overgeared-like: Kitsu lists 8 bare numbers (one titled "Episode 3"), ani.zip lists 12 rich ones, 8 with a
    /// screencap, the last 2 in the future.
    #[test]
    fn number_only_base_is_filled_and_extended_by_aired_episodes() {
        let mut kitsu: Vec<Episode> = (1..=8).map(bare).collect();
        kitsu[2].title = Some("Episode 3".into());
        kitsu[0].title = Some("Kitsu's own title".into());
        let anizip: Vec<Episode> = (1..=12)
            .map(|n| {
                let date = if n <= 10 { format!("2026-09-{:02}", n) } else { format!("2026-12-{:02}", n) };
                rich(n, &date, n <= 8)
            })
            .collect();
        let out = merge_episodes(kitsu, &anizip, "2026-11-15");
        // 8 from Kitsu + 9, 10 aired; 11, 12 not yet.
        assert_eq!(out.iter().map(|e| e.number).collect::<Vec<_>>(), (1..=10).collect::<Vec<_>>());
        // The base's real title stays; the placeholder is replaced.
        assert_eq!(out[0].title.as_deref(), Some("Kitsu's own title"));
        assert_eq!(out[2].title.as_deref(), Some("Chapter 3"));
        assert!(out[..8].iter().all(|e| e.image.is_some() && e.summary.is_some() && e.runtime_min == Some(24)));
        assert_eq!(out[4].rating, Some(7.5));
        assert!(out[8].image.is_none() && out[8].summary.is_some());
        assert!(!needs_fill(&out[..8]));
        assert!(needs_fill(&out));
    }

    #[test]
    fn base_fields_win_and_duplicates_keep_the_first() {
        let mut a = rich(1, "2026-01-01", true);
        a.title = Some("Base".into());
        a.rating = None;
        let b = Episode { title: Some("Other".into()), rating: Some(9.0), image: Some("x".into()), ..rich(1, "2025-01-01", true) };
        let out = merge_episodes(vec![a.clone(), Episode { title: Some("dup".into()), ..a.clone() }], &[b], "2026-11-15");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title.as_deref(), Some("Base"));
        assert_eq!(out[0].air_date.as_deref(), Some("2026-01-01"));
        assert_eq!(out[0].image, a.image);
        assert_eq!(out[0].rating, Some(9.0));
    }

    #[test]
    fn extra_without_a_date_is_not_added() {
        let out = merge_episodes(vec![bare(1)], &[bare(2), rich(3, "2026-11-15T10:00:00Z", true)], "2026-11-15");
        // Same day is not "before today"; undated is unknown.
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn placeholders() {
        assert!(placeholder_title("Episode 4", 4));
        assert!(placeholder_title(" ep 4 ", 4));
        assert!(placeholder_title("4", 4));
        assert!(!placeholder_title("Episode 4", 5));
        assert!(!placeholder_title("The Journey's End", 1));
    }
}
