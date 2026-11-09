//! AniList queries of Home / calendar (wave B, worker **b**; checklist `home.latest-episodes`, `home.top10`,
//! `home.upcoming`, `home.most-favorite`, `home.genre-cloud`, `home.calendar.schedule`, `home.followed-updates`).
//! Child module of `anilist`: it calls the private `post` of `mod.rs` like `search` / `details` / `lists` do.
//! Planned: `AniList::airing_schedules(from_unix, to_unix, not_yet_aired, page)` (`Page.airingSchedules`),
//! `AniList::genres()` (`GenreCollection`), `AniList::top(range)` (`TRENDING_DESC` for today, `POPULARITY_DESC` over
//! the season for week / month: AniList has no time-boxed trending, b documents the approximation),
//! `AniList::upcoming(page)` (`status: NOT_YET_RELEASED`, `sort: START_DATE`), `AniList::most_favourite(page)`
//! (`sort: FAVOURITES_DESC`), `AniList::genre_feed(genre, page)`.

