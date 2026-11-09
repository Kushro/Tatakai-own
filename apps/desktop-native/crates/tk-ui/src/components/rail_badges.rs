//! Rail badges (wave B): `use_rail_badges` polls the owners' counters and hands `NavRail { badges }` the non-zero
//! ones. Library = AniList unread notifications (worker n, `anilist_unread_count`, every
//! `Settings::social.notifications_poll_minutes`); Extensions = updates found (worker j, `ext_updates_check`, cached a
//! day, polled hourly); Downloads = active rows (worker h, `downloads`, every 10 s while any is active, else every
//! minute). Errors (incl. `NotWired`) count as 0, so nothing shows until each owner lands. Shared file: owners do not
//! edit it; they only implement the backend calls.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::model::NavDest;

const UPDATES_POLL: Duration = Duration::from_secs(60 * 60);
const DOWNLOADS_IDLE_POLL: Duration = Duration::from_secs(60);
const DOWNLOADS_ACTIVE_POLL: Duration = Duration::from_secs(10);

/// `(destination, count)` pairs for `NavRail::badges`; zero counts are dropped.
pub fn use_rail_badges() -> Vec<(NavDest, u32)> {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let mut unread = use_signal(|| 0_u32);
    let mut updates = use_signal(|| 0_u32);
    let mut active = use_signal(|| 0_u32);

    // n: AniList unread count (rail badge, `social.notifications.badge`).
    {
        let backend = services.backend.clone();
        use_future(move || {
            let backend = backend.clone();
            async move {
                loop {
                    let (poll_minutes, badge) = {
                        let s = ui.settings.peek();
                        (s.social.notifications_poll_minutes, s.social.badge)
                    };
                    if poll_minutes == 0 || !badge {
                        unread.set(0);
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                    let n = backend.anilist_unread_count().await.unwrap_or(0);
                    if *unread.peek() != n {
                        unread.set(n);
                    }
                    tokio::time::sleep(Duration::from_secs(60 * u64::from(poll_minutes))).await;
                }
            }
        });
    }
    // j: extension updates (`ext.updates.check`).
    {
        let backend = services.backend.clone();
        use_future(move || {
            let backend = backend.clone();
            async move {
                loop {
                    let n = if ui.settings.peek().ext.auto_check_updates {
                        backend.ext_updates_check().await.map(|v| v.len() as u32).unwrap_or(0)
                    } else {
                        0
                    };
                    if *updates.peek() != n {
                        updates.set(n);
                    }
                    tokio::time::sleep(UPDATES_POLL).await;
                }
            }
        });
    }
    // h: active downloads.
    {
        let backend = services.backend.clone();
        use_future(move || {
            let backend = backend.clone();
            async move {
                loop {
                    let n = backend
                        .downloads()
                        .await
                        .map(|rows| rows.iter().filter(|d| d.state.is_active()).count() as u32)
                        .unwrap_or(0);
                    if *active.peek() != n {
                        active.set(n);
                    }
                    tokio::time::sleep(if n > 0 { DOWNLOADS_ACTIVE_POLL } else { DOWNLOADS_IDLE_POLL }).await;
                }
            }
        });
    }

    [(NavDest::Library, unread()), (NavDest::Extensions, updates()), (NavDest::Downloads, active())]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .collect()
}
