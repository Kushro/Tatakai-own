//! AniList manga / novel queries (wave B, worker **l**; worker m reads `NOVEL` through the same functions; checklist
//! `manga.metadata.anilist`, `manga.search`, `manga.details`, `manga.readlist*`, `novel.search`, `novel.details`).
//! Child module of `anilist` (private `post`). Planned: `AniList::manga_search(&SearchVars, format: Option<"NOVEL">)`
//! (`type: MANGA`, `countryOfOrigin` for JP / KR / CN), `AniList::manga_extra(&MediaRef)` (chapters, volumes, staff
//! authors, relations incl. `ADAPTATION` -> anime id, external links), and the manga list (`MediaListCollection(type:
//! MANGA)`, `SaveMediaListEntry` with `progress` = chapters / `progressVolumes`) reusing `lists.rs` helpers of worker g.

