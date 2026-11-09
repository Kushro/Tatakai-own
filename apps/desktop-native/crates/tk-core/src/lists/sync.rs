//! Pure list-sync rules (wave A, worker **g**): the three-way merge between the user's local list (`store` rows
//! with `service = "local"`), the last state seen on a service (`service = "anilist" | "mal"`, the *base*) and the
//! service's current state, the edit rule of `Backend::set_list_entry` and the automatic-progress rule of
//! `Backend::record_progress`. Ported from the classic `src/lib/listSync.ts::diffLists` (remote only -> import,
//! local only -> export, same status with different progress -> higher progress wins, different status ->
//! conflict) and extended with the base so a change made on only one side never becomes a conflict.
//! No I/O: tk-app's `live/lists.rs` runs the network and store calls around these functions.

use serde::{Deserialize, Serialize};

use super::{Service, score_from_mal, score_to_mal};
use crate::store::lists::ListEntryRow;

/// `service` column of the user's own list (works without any account: `lists.local-only`).
pub const LOCAL: &str = "local";

/// The fields that make two rows "the same entry state" for `service` (MAL scores have 10 steps).
fn state(row: &ListEntryRow, service: Service) -> (&str, u32, Option<u8>) {
    let score = match service {
        Service::AniList => row.score.filter(|s| *s > 0),
        Service::Mal => score_from_mal(score_to_mal(row.score)),
    };
    (row.status.as_str(), row.progress, score)
}

pub fn same(a: &ListEntryRow, b: &ListEntryRow, service: Service) -> bool {
    state(a, service) == state(b, service)
}

/// What one title needs after comparing the three sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Already equal (or a pending local write will settle it): just remember the remote as the base.
    Nothing,
    /// Copy the remote state into the local list.
    TakeRemote,
    /// Queue a push of the local state to the service.
    PushLocal,
    /// The title was removed on the service and untouched locally: remove it locally.
    DeleteLocal,
    /// The title was removed locally and untouched on the service: queue a remote delete.
    DeleteRemote,
    /// Both sides changed in incompatible ways: ask the user (`list_conflicts`).
    Conflict,
}

/// Three-way decision. `pending` = a queued write or an open conflict exists for this title and service.
pub fn decide(
    local: Option<&ListEntryRow>,
    base: Option<&ListEntryRow>,
    remote: Option<&ListEntryRow>,
    pending: bool,
    service: Service,
) -> Decision {
    if pending {
        return Decision::Nothing;
    }
    match (local, remote) {
        (None, None) => Decision::Nothing,
        (None, Some(r)) => match base {
            Some(b) if same(b, r, service) => Decision::DeleteRemote,
            _ => Decision::TakeRemote,
        },
        (Some(l), None) => match base {
            Some(b) if same(l, b, service) => Decision::DeleteLocal,
            _ => Decision::PushLocal,
        },
        (Some(l), Some(r)) => {
            if same(l, r, service) {
                return Decision::Nothing;
            }
            if let Some(b) = base {
                if same(l, b, service) {
                    return Decision::TakeRemote;
                }
                if same(r, b, service) {
                    return Decision::PushLocal;
                }
            }
            if l.status == r.status {
                // Classic rule: only progress (or score) differs -> the side that is ahead wins.
                if r.progress != l.progress {
                    return if r.progress > l.progress {
                        Decision::TakeRemote
                    } else {
                        Decision::PushLocal
                    };
                }
                return if r.updated_at > l.updated_at {
                    Decision::TakeRemote
                } else {
                    Decision::PushLocal
                };
            }
            Decision::Conflict
        }
    }
}

/// The local row after taking `remote` (keeps what the remote does not know: title, poster, ids).
pub fn merged_local(local: Option<&ListEntryRow>, remote: &ListEntryRow) -> ListEntryRow {
    let pick = |r: &Option<String>, l: Option<&Option<String>>| r.clone().or_else(|| l.cloned().flatten());
    ListEntryRow {
        service: LOCAL.to_owned(),
        media_id: local.map_or_else(|| remote.media_id.clone(), |l| l.media_id.clone()),
        anilist_id: remote.anilist_id.or(local.and_then(|l| l.anilist_id)),
        mal_id: remote.mal_id.or(local.and_then(|l| l.mal_id)),
        title: if remote.title.is_empty() {
            local.map(|l| l.title.clone()).unwrap_or_default()
        } else {
            remote.title.clone()
        },
        poster: pick(&remote.poster, local.map(|l| &l.poster)),
        status: remote.status.clone(),
        progress: remote.progress,
        episodes: remote.episodes.or(local.and_then(|l| l.episodes)),
        score: remote.score.filter(|s| *s > 0),
        started_at: pick(&remote.started_at, local.map(|l| &l.started_at)),
        finished_at: pick(&remote.finished_at, local.map(|l| &l.finished_at)),
        repeats: remote.repeats,
        notes: remote.notes.clone(),
        remote_id: None,
        updated_at: remote.updated_at,
        dirty: false,
    }
}

/// A partial edit (`ListEntryUpdateVm` in tk-ui terms). `score: Some(0)` clears the score; `notes: Some("")` clears
/// the notes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryPatch {
    pub status: Option<String>,
    pub progress: Option<u32>,
    pub score: Option<u8>,
    pub notes: Option<String>,
}

/// Apply `patch` to `row` (a new row comes in with an empty status). Rules: a new entry without a status is
/// `planning` (or `watching` when it gets progress); progress is capped at the episode count; reaching the last
/// episode while watching / repeating completes the entry (repeating also counts one more rewatch); the start and
/// finish dates are filled the first time; the row becomes dirty with `updated_at = now`.
pub fn apply_patch(row: &mut ListEntryRow, patch: &EntryPatch, today: &str, now: i64) {
    if let Some(s) = &patch.status {
        if super::STATUSES.contains(&s.as_str()) {
            row.status = s.clone();
        }
    }
    if let Some(p) = patch.progress {
        row.progress = match row.episodes {
            Some(total) if total > 0 => p.min(total),
            _ => p,
        };
    }
    if row.status.is_empty() {
        row.status = if row.progress > 0 { "watching" } else { "planning" }.to_owned();
    }
    if patch.status.is_none()
        && patch.progress.is_some()
        && row.progress > 0
        && matches!(row.status.as_str(), "planning" | "paused" | "dropped")
    {
        row.status = "watching".to_owned();
    }
    if patch.status.as_deref() == Some("completed")
        && let Some(total) = row.episodes.filter(|t| *t > 0)
    {
        row.progress = total;
    }
    if let Some(total) = row.episodes.filter(|t| *t > 0)
        && row.progress >= total
        && patch.status.is_none()
    {
        match row.status.as_str() {
            "watching" => row.status = "completed".to_owned(),
            "repeating" => {
                row.status = "completed".to_owned();
                row.repeats += 1;
            }
            _ => {}
        }
    }
    if let Some(score) = patch.score {
        row.score = (score > 0).then(|| score.min(100));
    }
    if let Some(n) = &patch.notes {
        let n = n.trim();
        row.notes = (!n.is_empty()).then(|| n.to_owned());
    }
    if matches!(row.status.as_str(), "watching" | "repeating" | "completed") && row.started_at.is_none() {
        row.started_at = Some(today.to_owned());
    }
    if row.status == "completed" && row.finished_at.is_none() {
        row.finished_at = Some(today.to_owned());
    }
    row.updated_at = now;
    row.dirty = true;
}

/// Automatic progress after an episode counted as watched (`lists.auto-progress`, `lists.auto-add`).
/// `current` = `(status, progress)` of the local entry, `None` when the title is not in the list (it is added as
/// `watching`). `None` result = nothing to change: the episode is not ahead of the progress, or the entry is
/// completed (a rewatch outside the list does not reopen it).
pub fn auto_progress(
    current: Option<(&str, u32)>,
    episode: u32,
    episodes_total: Option<u32>,
) -> Option<EntryPatch> {
    if episode == 0 {
        return None;
    }
    let finished = episodes_total.is_some_and(|t| t > 0 && episode >= t);
    match current {
        None => Some(EntryPatch {
            status: Some(if finished { "completed" } else { "watching" }.to_owned()),
            progress: Some(episode),
            ..EntryPatch::default()
        }),
        Some((status, progress)) => {
            if episode <= progress || status == "completed" {
                return None;
            }
            let status = match status {
                _ if finished && status != "repeating" => Some("completed".to_owned()),
                "planning" | "paused" | "dropped" => Some("watching".to_owned()),
                _ => None,
            };
            Some(EntryPatch {
                status,
                progress: Some(episode),
                ..EntryPatch::default()
            })
        }
    }
}

/// Whether `row` can be written to `service` (it needs that service's id).
pub fn addressable(row: &ListEntryRow, service: Service) -> bool {
    match service {
        Service::AniList => row.anilist_id.is_some(),
        Service::Mal => row.mal_id.is_some(),
    }
}

/// UI media id of a title: the AniList id as text, else `mal:<id>`.
pub fn media_id_for(anilist_id: Option<u32>, mal_id: Option<u32>) -> Option<String> {
    match (anilist_id, mal_id) {
        (Some(a), _) => Some(a.to_string()),
        (None, Some(m)) => Some(format!("mal:{m}")),
        _ => None,
    }
}

/// JSON form of a row: conflict sides (`list_conflicts.local_json / remote_json`), queue payloads and the
/// Tatakai JSON export / import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RowJson {
    pub media_id: String,
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
    pub title: String,
    pub poster: Option<String>,
    pub status: String,
    pub progress: u32,
    pub episodes: Option<u32>,
    pub score: Option<u8>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub repeats: u32,
    pub notes: Option<String>,
    pub remote_id: Option<u64>,
    pub updated_at: i64,
}

impl Default for RowJson {
    fn default() -> Self {
        Self {
            media_id: String::new(),
            anilist_id: None,
            mal_id: None,
            title: String::new(),
            poster: None,
            status: "planning".to_owned(),
            progress: 0,
            episodes: None,
            score: None,
            started_at: None,
            finished_at: None,
            repeats: 0,
            notes: None,
            remote_id: None,
            updated_at: 0,
        }
    }
}

impl From<&ListEntryRow> for RowJson {
    fn from(r: &ListEntryRow) -> Self {
        Self {
            media_id: r.media_id.clone(),
            anilist_id: r.anilist_id,
            mal_id: r.mal_id,
            title: r.title.clone(),
            poster: r.poster.clone(),
            status: r.status.clone(),
            progress: r.progress,
            episodes: r.episodes,
            score: r.score,
            started_at: r.started_at.clone(),
            finished_at: r.finished_at.clone(),
            repeats: r.repeats,
            notes: r.notes.clone(),
            remote_id: r.remote_id,
            updated_at: r.updated_at,
        }
    }
}

impl RowJson {
    pub fn into_row(self, service: &str, dirty: bool) -> ListEntryRow {
        ListEntryRow {
            service: service.to_owned(),
            media_id: self.media_id,
            anilist_id: self.anilist_id,
            mal_id: self.mal_id,
            title: self.title,
            poster: self.poster,
            status: self.status,
            progress: self.progress,
            episodes: self.episodes,
            score: self.score,
            started_at: self.started_at,
            finished_at: self.finished_at,
            repeats: self.repeats,
            notes: self.notes,
            remote_id: self.remote_id,
            updated_at: self.updated_at,
            dirty,
        }
    }

    pub fn to_json(row: &ListEntryRow) -> String {
        serde_json::to_string(&RowJson::from(row)).unwrap_or_else(|_| "{}".to_owned())
    }

    pub fn parse(json: &str, service: &str) -> Option<ListEntryRow> {
        serde_json::from_str::<RowJson>(json)
            .ok()
            .map(|r| r.into_row(service, false))
    }
}

#[cfg(test)]
pub(crate) fn row(service: &str, media: &str, status: &str, progress: u32, updated_at: i64) -> ListEntryRow {
    ListEntryRow {
        service: service.to_owned(),
        media_id: media.to_owned(),
        anilist_id: media.parse().ok(),
        mal_id: None,
        title: format!("T{media}"),
        poster: None,
        status: status.to_owned(),
        progress,
        episodes: Some(12),
        score: None,
        started_at: None,
        finished_at: None,
        repeats: 0,
        notes: None,
        remote_id: None,
        updated_at,
        dirty: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Service = Service::AniList;

    #[test]
    fn one_sided_changes_never_conflict() {
        let base = row("anilist", "1", "watching", 3, 10);
        let local_changed = row(LOCAL, "1", "paused", 3, 20);
        let remote_same = row("anilist", "1", "watching", 3, 10);
        assert_eq!(
            decide(Some(&local_changed), Some(&base), Some(&remote_same), false, A),
            Decision::PushLocal
        );
        let local_same = row(LOCAL, "1", "watching", 3, 10);
        let remote_changed = row("anilist", "1", "dropped", 3, 30);
        assert_eq!(
            decide(Some(&local_same), Some(&base), Some(&remote_changed), false, A),
            Decision::TakeRemote
        );
    }

    #[test]
    fn classic_rules_without_a_base() {
        let r = row("anilist", "1", "watching", 5, 10);
        assert_eq!(
            decide(None, None, Some(&r), false, A),
            Decision::TakeRemote,
            "remote only -> import"
        );
        let l = row(LOCAL, "1", "watching", 2, 10);
        assert_eq!(
            decide(Some(&l), None, None, false, A),
            Decision::PushLocal,
            "local only -> export"
        );
        assert_eq!(
            decide(Some(&l), None, Some(&r), false, A),
            Decision::TakeRemote,
            "remote ahead"
        );
        let l7 = row(LOCAL, "1", "watching", 7, 10);
        assert_eq!(
            decide(Some(&l7), None, Some(&r), false, A),
            Decision::PushLocal,
            "local ahead"
        );
        let lp = row(LOCAL, "1", "paused", 5, 10);
        assert_eq!(
            decide(Some(&lp), None, Some(&r), false, A),
            Decision::Conflict,
            "status differs"
        );
        assert_eq!(
            decide(Some(&lp), None, Some(&r), true, A),
            Decision::Nothing,
            "pending write wins"
        );
        assert_eq!(
            decide(Some(&r.clone()), None, Some(&r), false, A),
            Decision::Nothing
        );
    }

    #[test]
    fn both_changed_differently_is_a_conflict() {
        let base = row("anilist", "1", "watching", 3, 10);
        let l = row(LOCAL, "1", "paused", 4, 20);
        let r = row("anilist", "1", "completed", 12, 30);
        assert_eq!(
            decide(Some(&l), Some(&base), Some(&r), false, A),
            Decision::Conflict
        );
    }

    #[test]
    fn deletions_follow_the_base() {
        let base = row("anilist", "1", "watching", 3, 10);
        let same_local = row(LOCAL, "1", "watching", 3, 10);
        assert_eq!(
            decide(Some(&same_local), Some(&base), None, false, A),
            Decision::DeleteLocal
        );
        let changed_local = row(LOCAL, "1", "watching", 4, 11);
        assert_eq!(
            decide(Some(&changed_local), Some(&base), None, false, A),
            Decision::PushLocal
        );
        let r = row("anilist", "1", "watching", 3, 10);
        assert_eq!(
            decide(None, Some(&base), Some(&r), false, A),
            Decision::DeleteRemote
        );
        let r2 = row("anilist", "1", "watching", 6, 12);
        assert_eq!(
            decide(None, Some(&base), Some(&r2), false, A),
            Decision::TakeRemote
        );
    }

    #[test]
    fn mal_compares_scores_in_tens() {
        let mut l = row(LOCAL, "1", "watching", 3, 10);
        let mut r = row("mal", "1", "watching", 3, 10);
        l.score = Some(84);
        r.score = Some(80);
        assert!(same(&l, &r, Service::Mal));
        assert!(!same(&l, &r, Service::AniList));
    }

    #[test]
    fn merged_local_keeps_what_the_remote_lacks() {
        let mut l = row(LOCAL, "1", "watching", 3, 10);
        l.poster = Some("p".into());
        let mut r = row("anilist", "1", "completed", 12, 30);
        r.title = String::new();
        r.remote_id = Some(99);
        let m = merged_local(Some(&l), &r);
        assert_eq!(
            (m.service.as_str(), m.title.as_str(), m.poster.as_deref()),
            (LOCAL, "T1", Some("p"))
        );
        assert_eq!(
            (m.status.as_str(), m.progress, m.remote_id, m.dirty),
            ("completed", 12, None, false)
        );
    }

    #[test]
    fn patch_rules() {
        let mut r = row(LOCAL, "1", "", 0, 0);
        apply_patch(&mut r, &EntryPatch::default(), "2026-11-09", 5);
        assert_eq!(r.status, "planning");
        assert!(r.dirty);
        apply_patch(
            &mut r,
            &EntryPatch {
                progress: Some(3),
                ..Default::default()
            },
            "2026-11-09",
            6,
        );
        assert_eq!(
            (r.status.as_str(), r.progress, r.started_at.as_deref()),
            ("watching", 3, Some("2026-11-09"))
        );
        apply_patch(
            &mut r,
            &EntryPatch {
                progress: Some(40),
                ..Default::default()
            },
            "2026-11-10",
            7,
        );
        assert_eq!(
            (r.status.as_str(), r.progress, r.finished_at.as_deref()),
            ("completed", 12, Some("2026-11-10"))
        );
        apply_patch(
            &mut r,
            &EntryPatch {
                score: Some(85),
                ..Default::default()
            },
            "x",
            8,
        );
        assert_eq!(r.score, Some(85));
        apply_patch(
            &mut r,
            &EntryPatch {
                score: Some(0),
                notes: Some("  ".into()),
                ..Default::default()
            },
            "x",
            9,
        );
        assert_eq!((r.score, r.notes.clone()), (None, None));
        let mut rep = row(LOCAL, "2", "repeating", 11, 0);
        apply_patch(
            &mut rep,
            &EntryPatch {
                progress: Some(12),
                ..Default::default()
            },
            "x",
            1,
        );
        assert_eq!((rep.status.as_str(), rep.repeats), ("completed", 1));
        let mut c = row(LOCAL, "3", "watching", 2, 0);
        apply_patch(
            &mut c,
            &EntryPatch {
                status: Some("completed".into()),
                ..Default::default()
            },
            "x",
            1,
        );
        assert_eq!(c.progress, 12, "completing fills the progress");
        apply_patch(
            &mut c,
            &EntryPatch {
                status: Some("bogus".into()),
                ..Default::default()
            },
            "x",
            2,
        );
        assert_eq!(c.status, "completed", "unknown statuses are ignored");
    }

    #[test]
    fn auto_progress_rules() {
        let p = auto_progress(None, 1, Some(12)).unwrap();
        assert_eq!(
            (p.status.as_deref(), p.progress),
            (Some("watching"), Some(1)),
            "auto-add as watching"
        );
        assert_eq!(
            auto_progress(None, 1, Some(1)).unwrap().status.as_deref(),
            Some("completed")
        );
        assert_eq!(
            auto_progress(Some(("watching", 3)), 3, Some(12)),
            None,
            "not ahead"
        );
        let p = auto_progress(Some(("watching", 3)), 4, Some(12)).unwrap();
        assert_eq!((p.status, p.progress), (None, Some(4)));
        assert_eq!(
            auto_progress(Some(("planning", 0)), 1, None)
                .unwrap()
                .status
                .as_deref(),
            Some("watching")
        );
        assert_eq!(
            auto_progress(Some(("watching", 11)), 12, Some(12))
                .unwrap()
                .status
                .as_deref(),
            Some("completed")
        );
        assert_eq!(auto_progress(Some(("completed", 12)), 13, Some(24)), None);
        assert_eq!(
            auto_progress(Some(("repeating", 2)), 3, Some(12)).unwrap().status,
            None
        );
        assert_eq!(auto_progress(None, 0, None), None);
    }

    #[test]
    fn row_json_round_trip() {
        let mut r = row("anilist", "21", "watching", 3, 10);
        r.notes = Some("n".into());
        let back = RowJson::parse(&RowJson::to_json(&r), "anilist").unwrap();
        assert_eq!(back, r);
        assert_eq!(media_id_for(Some(1), Some(2)).as_deref(), Some("1"));
        assert_eq!(media_id_for(None, Some(2)).as_deref(), Some("mal:2"));
        assert_eq!(media_id_for(None, None), None);
        assert!(addressable(&r, Service::AniList) && !addressable(&r, Service::Mal));
    }
}
