# Supabase tables used by M0 (no migrations in M0)

The native apps talk to the SAME own Supabase project as the legacy app. **M0 adds no migrations.** Any future migration must be
idempotent and re-runnable (`create ... if not exists`, `drop policy if exists`, `add column if not exists`).
Sources of truth: `supabase/main.sql` (schema snapshot), `supabase/migrations/20251231031018_remix_migration_from_pg_dump.sql`
(constraints/policies), `supabase/migrations/20260128000001_add_mal_anilist_ids.sql` (ids). Client config arrives via `TATAKAI_SUPABASE_URL` /
`TATAKAI_SUPABASE_ANON_KEY` (never committed; see `apps/README.md`).

Auth: GoTrue email + password only in M0 (`POST {url}/auth/v1/token?grant_type=password`, refresh with `grant_type=refresh_token`).
All data requests send `apikey: <anon key>` and `Authorization: Bearer <access_token>`. The session is NEVER sent to any host other than the Supabase URL.

## 1. `public.watch_history` (literal, `supabase/main.sql:1710`)

```sql
CREATE TABLE public.watch_history (
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  anime_id text NOT NULL,
  anime_name text NOT NULL,
  anime_poster text,
  episode_id text NOT NULL,
  episode_number integer NOT NULL,
  progress_seconds integer DEFAULT 0,
  duration_seconds integer,
  completed boolean DEFAULT false,
  watched_at timestamp with time zone NOT NULL DEFAULT now(),
  mal_id integer,
  anilist_id integer,
  CONSTRAINT watch_history_pkey PRIMARY KEY (id),
  CONSTRAINT watch_history_user_id_fkey FOREIGN KEY (user_id) REFERENCES auth.users(id)   -- ON DELETE CASCADE in the live DB
);
```

Constraints / indexes / RLS that matter (from the migrations):

| Item | Value |
|---|---|
| Upsert key | `UNIQUE (user_id, episode_id)` = `watch_history_user_id_episode_id_key` |
| Indexes | `idx_watch_history_user_id (user_id)`, `idx_watch_history_watched_at (watched_at DESC)`, `(user_id, watched_at DESC)`, `idx_watch_history_mal_id`, `idx_watch_history_anilist_id` |
| RLS (own rows) | `Users can view own history` (SELECT), `Users can insert own history` (INSERT, `auth.uid() = user_id`), `Users can update own history` (UPDATE), `Users can delete own history` (DELETE) |
| RLS (public) | `Allow viewing public history` (SELECT when the owner's profile has `show_history`/`is_public`); irrelevant to the native client, which only touches its own rows |

### 1.1 The exact keys the legacy app writes (E9) - the native apps MUST write the same ones

Verified in `src/hooks/user/useWatchHistory.ts` (`useUpdateWatchHistory`), `src/pages/watch/WatchPage.tsx` (route `/watch/:episodeId`, `animeId = episodeId.split('?')[0]`),
`src/core/content/types.ts` (`toAnimeCard`), `src/contexts/AuthContext.tsx` (local->DB migration upsert):

| Column | Legacy value | Native rule |
|---|---|---|
| `anime_id` | `String(anilistId)` when `anilistId > 0` (e.g. `"154587"`); otherwise `"mal:<malId>"`; (very old rows may hold a provider slug). It is the id used by the web route `/anime/:id`. | Always `String(anilistId)` when known. Resolve via the metadata router (ani.zip) before writing; if only MAL is known write `"mal:<malId>"`. Never write Kitsu ids. |
| `episode_id` | `` `${anime_id}?ep=${episode_number}` `` e.g. `"154587?ep=3"` (this string is the web URL segment `/watch/154587%3Fep%3D3`). Torrent sessions use `torrent-<infohash>?ep=1` (not produced by M0). | Exactly `"<anime_id>?ep=<episode_number>"`, no encoding, no padding. |
| `episode_number` | integer, 1-based, absolute within the AniList entry | same |
| `anime_name` | display title (`animeData.info.name`), `'Unknown Anime'` if missing | `Media.title` (English, fallback romaji) |
| `anime_poster` | poster URL string or `''` | `Media.cover` or `''` |
| `progress_seconds` | `Math.round(progress)` | rounded int |
| `duration_seconds` | `Math.round(duration)` (0 when unknown at "started" seed time) | rounded int, `null`/0 if unknown |
| `completed` | `completed OR (duration > 0 AND progress/duration >= 0.9)` | same 90 % rule |
| `watched_at` | `new Date().toISOString()` | UTC ISO-8601 at write time |
| `mal_id` / `anilist_id` | **only included in the payload when non-null** (never overwrite an existing id with null) | same: omit the keys when unknown |

Behavioural rules the native clients copy (so both apps show the same "Continue watching"):

* "Continue watching" = `GET /rest/v1/watch_history?select=*&user_id=eq.<uid>&order=watched_at.desc&limit=30`, then client-side: drop `completed = true`, keep
  ONE row per `anime_id` (the most recently watched episode), show up to 10 (home row uses 20 in native).
* Merge local vs remote by `episode_id`, newest `watched_at` wins; on a tie, higher `progress_seconds` wins.
* A "started watching" row (progress 0) is only written once playback really began (legacy waits 5 s and checks position > 0), so it never overwrites a real position from another device.
* Local-first: write Room/SQLite immediately; sync is coalesced to at most one request per 15 s per episode, plus on pause and on leaving the player.

### 1.2 PostgREST calls (both clients)

```
# upsert (batch ok)
POST {url}/rest/v1/watch_history?on_conflict=user_id,episode_id
apikey: <anon>            Authorization: Bearer <access_token>
Content-Type: application/json
Prefer: resolution=merge-duplicates,return=minimal
[{"user_id":"<uid>","anime_id":"154587","anime_name":"Frieren: Beyond Journey's End","anime_poster":"https://...",
  "episode_id":"154587?ep=3","episode_number":3,"progress_seconds":812,"duration_seconds":1440,"completed":false,
  "watched_at":"2026-10-27T21:04:11.000Z","anilist_id":154587,"mal_id":52991}]

# read
GET {url}/rest/v1/watch_history?select=*&user_id=eq.<uid>&order=watched_at.desc&limit=30
```

`user_id` MUST equal the token's `sub` (RLS `WITH CHECK auth.uid() = user_id`). A 401/`PGRST301` means refresh the token once, then surface "signed out".

## 2. `public.profiles` (read-only in M0)

Literal definition: `supabase/main.sql:1223` (columns: `id, user_id (UNIQUE), username, display_name, avatar_url, bio, created_at, updated_at, is_admin, is_banned, ..., is_public, banner_url,
total_watch_time_seconds, showcase_anime_ids, mal_user_id, mal_access_token, mal_refresh_token, mal_token_expires_at, anilist_user_id, anilist_access_token, ... preferred_title_language, app_settings, ...`).

**Native clients select explicit columns only**, never `*`:

```
GET {url}/rest/v1/profiles?select=user_id,username,display_name,avatar_url,preferred_title_language,is_banned&user_id=eq.<uid>
```

Why: `profiles` is world-readable (policy `USING true`) and legacy rows may still carry `mal_*`/`anilist_*` token columns. Since commit `b80d649` the tokens live in the owner-only table
`public.user_integration_tokens` (RLS owner-only, no anon access); **M0 does not read or write it** (AniList/MAL linking is M1). If `is_banned = true` the app shows "account suspended" and does not sync.
Writes to `profiles` are out of scope in M0 (display name edits come in M1).

## 3. Not used in M0

`watchlist`, `watch_rooms*`, `user_integration_tokens`, `comments`, community tables, `app_settings` - all M1+.
