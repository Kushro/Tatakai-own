/**
 * MyAnimeList integration (client side).
 *
 * Login is OAuth2 + PKCE with a public client id (see `@/lib/mal/auth`); list
 * reads and writes are MAL REST v2 calls made straight from the app with the
 * user's own token. Nothing goes through TatakaiAPI and the Supabase session
 * never leaves Supabase.
 */

import {
    MAL_API_URL,
    MAL_DEFAULT_REDIRECT_URI,
    MalAuthExpiredError,
    clearMalCredentials,
    clearMalLoginStorage,
    clearMalPendingMarker as clearPendingMarker,
    completeMalLogin,
    isMalLoginPending,
    malFetch,
    malJson,
    startMalLogin,
} from '@/lib/mal/auth';

export { MalAuthExpiredError };

/** The redirect URL to register in the MAL app settings (and the build default). */
export const MAL_CANONICAL_REDIRECT_URI = MAL_DEFAULT_REDIRECT_URI;

/**
 * Generates the MAL authorization URL (PKCE, plain challenge). Also stores the
 * code_verifier/state and marks a login as in flight.
 */
export function getMalAuthUrl(redirectUriOverride?: string, _stateOverride?: string): string {
    return startMalLogin(redirectUriOverride);
}

/**
 * True when a login was started recently but its callback never ran. Starting a
 * new flow now would overwrite the stored verifier and doom the old attempt.
 */
export function isMalLoginPendingFresh(): boolean {
    return isMalLoginPending();
}

/** Clears a pending/in-flight login so a brand-new flow can start. */
export function clearMalOAuthStorage(): void {
    clearMalLoginStorage();
}

/** Clears just the in-flight marker (the flow reached the callback page and is over). */
export function clearMalPendingMarker(): void {
    clearPendingMarker();
}

/**
 * Kept for the settings panel. The tatakai:// redirect has no origin to compare,
 * so there is nothing to assert any more.
 */
export function assertWebMalRedirectMatchesOrigin(_redirectUri: string): void {
    /* tatakai:// deep link: no web origin involved */
}

/**
 * Completes the MAL authentication: validates `state`, exchanges the code for
 * tokens (PKCE, no secret) and stores them on the user's profile.
 */
export async function exchangeMalCode(code: string, state?: string | null) {
    const result = await completeMalLogin({ code, state: state ?? null });
    return result;
}

/**
 * Resolves a MAL ID by fetching the first episode of an anime.
 * This is used as a fallback when the ID is missing in our database.
 */
export async function resolveMalIdFromEpisodes(animeId: string): Promise<number | null> {
    try {
        const { fetchEpisodes, fetchTatakaiEpisodeSources, fetchCombinedSources } = await import('@/lib/api');

        const { episodes } = await fetchEpisodes(animeId);
        if (!episodes || episodes.length === 0) return null;

        const firstEpisode = episodes[0];
        const sourceData = await fetchTatakaiEpisodeSources(firstEpisode.episodeId);
        if (sourceData?.malID) return Number(sourceData.malID);

        const combined = await fetchCombinedSources(firstEpisode.episodeId, animeId, 1, 'hd-2', 'sub');
        if (combined?.malID) return Number(combined.malID);
        return null;
    } catch (error) {
        console.error('[MAL Sync] Fallback - Error during resolution:', error);
        return null;
    }
}

// ── Status mapping ───────────────────────────────────────────────────────────

const MAL_ANIME_STATUSES = new Set(['watching', 'completed', 'on_hold', 'dropped', 'plan_to_watch']);
const MAL_MANGA_STATUSES = new Set(['reading', 'completed', 'on_hold', 'dropped', 'plan_to_read']);

/** AniList names -> Tatakai/MAL names, so either vocabulary can be passed in. */
const ALIASES: Record<string, string> = {
    current: 'watching', repeating: 'watching', planning: 'plan_to_watch', paused: 'on_hold',
};

/** Maps a Tatakai (or AniList) anime status to MAL's. */
export function mapTatakaiStatusToMal(status: string): string {
    const normalized = String(status || '').trim().toLowerCase();
    const value = ALIASES[normalized] ?? normalized;
    return MAL_ANIME_STATUSES.has(value) ? value : 'plan_to_watch';
}

/**
 * Maps MAL status strings to Tatakai status strings.
 */
export function mapMalStatusToTatakai(malStatus: string): any {
    return mapTatakaiStatusToMal(malStatus);
}

/**
 * Maps MAL manga status strings to local manga readlist status strings.
 */
export function mapMalMangaStatusToTatakai(malStatus: string): any {
    const normalized = String(malStatus || '').trim().toLowerCase();
    const value = normalized === 'current' || normalized === 'repeating' ? 'reading' : normalized === 'planning' ? 'plan_to_read' : normalized === 'paused' ? 'on_hold' : normalized;
    return MAL_MANGA_STATUSES.has(value) ? value : 'plan_to_read';
}

/**
 * Maps local manga readlist status strings to MAL manga status strings.
 */
export function mapTatakaiMangaStatusToMal(status: string): any {
    return mapMalMangaStatusToTatakai(status);
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function parseMalNumericId(value: string | number): number | null {
    if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
        return Math.trunc(value);
    }

    const raw = String(value || '').trim();
    if (!raw) return null;

    if (/^\d+$/.test(raw)) {
        return parseInt(raw, 10);
    }

    const prefixed = raw.match(/^mal[:-](\d+)$/i);
    if (prefixed?.[1]) {
        return parseInt(prefixed[1], 10);
    }

    return null;
}

/** MAL scores are integers 0-10 (0 = unscored). */
function malScore(score: number | undefined): number | undefined {
    if (score === undefined || !Number.isFinite(score)) return undefined;
    return Math.max(0, Math.min(10, Math.round(score)));
}

async function failIfNotOk(res: { ok: boolean; status: number; text(): Promise<string> }, what: string): Promise<void> {
    if (res.ok) return;
    const body = await res.text().catch(() => '');
    throw new Error(`${what} failed (${res.status})${body ? `: ${body.slice(0, 200)}` : ''}`);
}

/**
 * Fetch every page of a MAL list endpoint. `paging.next` is followed only while
 * it stays on api.myanimelist.net, so the bearer token cannot be redirected away.
 */
async function fetchAllMalPages(firstPath: string): Promise<any[]> {
    const out: any[] = [];
    let next: string | undefined = firstPath;
    for (let page = 0; next && page < 100; page++) {
        const data: any = await malJson<any>(next);
        out.push(...(data.data || []));
        const candidate: string | undefined = data.paging?.next;
        next = candidate && new URL(candidate).hostname === 'api.myanimelist.net' ? candidate : undefined;
    }
    return out;
}

async function resolveMalAnimeId(animeIdOrMalId: string | number): Promise<number> {
    if (typeof animeIdOrMalId === 'number') return animeIdOrMalId;
    if (/^\d+$/.test(animeIdOrMalId)) return parseInt(animeIdOrMalId, 10);
    const prefixed = parseMalNumericId(animeIdOrMalId);
    if (prefixed) return prefixed;

    const { supabase } = await import('@/integrations/supabase/client');
    const { data: { user } } = await supabase.auth.getUser();
    if (!user) throw new Error('Not authenticated');

    const { data: watchlistEntry } = await supabase
        .from('watchlist')
        .select('mal_id')
        .eq('user_id', user.id)
        .eq('anime_id', animeIdOrMalId)
        .maybeSingle();
    if (watchlistEntry?.mal_id) return watchlistEntry.mal_id;

    const { data: historyEntry } = await supabase
        .from('watch_history')
        .select('mal_id')
        .eq('user_id', user.id)
        .eq('anime_id', animeIdOrMalId)
        .order('watched_at', { ascending: false })
        .limit(1)
        .maybeSingle();
    if (historyEntry?.mal_id) return historyEntry.mal_id;

    const resolvedId = await resolveMalIdFromEpisodes(animeIdOrMalId);
    if (!resolvedId) throw new Error('MAL ID not found for this anime. Try watching an episode first.');
    // Persist for next time.
    await supabase.from('watchlist').update({ mal_id: resolvedId }).eq('user_id', user.id).eq('anime_id', animeIdOrMalId);
    await supabase.from('watch_history').update({ mal_id: resolvedId }).eq('user_id', user.id).eq('anime_id', animeIdOrMalId);
    return resolvedId;
}

async function resolveMalMangaId(mangaIdOrMalId: string | number): Promise<number> {
    const parsed = parseMalNumericId(mangaIdOrMalId);
    if (parsed) return parsed;

    const { supabase } = await import('@/integrations/supabase/client');
    const { data: { user } } = await supabase.auth.getUser();
    if (!user) throw new Error('Not authenticated');

    const mangaId = String(mangaIdOrMalId || '').trim();
    if (!mangaId) throw new Error('Missing manga ID');

    const { data: mappedByMangaId } = await supabase
        .from('manga_readlist')
        .select('mal_id')
        .eq('user_id', user.id)
        .eq('manga_id', mangaId)
        .maybeSingle();

    if (mappedByMangaId?.mal_id) {
        return Number(mappedByMangaId.mal_id);
    }

    const anilistPrefixed = mangaId.match(/^anilist:(\d+)$/i);
    if (anilistPrefixed?.[1]) {
        const { data: mappedByAniListId } = await supabase
            .from('manga_readlist')
            .select('mal_id')
            .eq('user_id', user.id)
            .eq('anilist_id', Number(anilistPrefixed[1]))
            .maybeSingle();

        if (mappedByAniListId?.mal_id) {
            return Number(mappedByAniListId.mal_id);
        }
    }

    throw new Error('MAL manga ID not found for this entry');
}

// ── Anime ────────────────────────────────────────────────────────────────────

/**
 * Updates the status of an anime on the user's MyAnimeList
 * (`PATCH /anime/{id}/my_list_status`).
 */
export async function updateMalAnimeStatus(
    animeIdOrMalId: string | number,
    status: string,
    score?: number,
    numWatchedEpisodes?: number
) {
    const malId = await resolveMalAnimeId(animeIdOrMalId);
    const res = await malFetch(`/anime/${malId}/my_list_status`, {
        method: 'PATCH',
        form: {
            status: mapTatakaiStatusToMal(status),
            score: malScore(score),
            num_watched_episodes: numWatchedEpisodes !== undefined ? Math.max(0, Math.trunc(numWatchedEpisodes)) : undefined,
        },
    });
    await failIfNotOk(res, 'MAL anime sync');
    return res.json();
}

/**
 * Fetches the user's entire anime list from MyAnimeList (every page).
 */
export async function fetchMalUserList() {
    return fetchAllMalPages('/users/@me/animelist?fields=list_status&limit=1000&nsfw=true');
}

/**
 * Removes an anime from the user's MyAnimeList library.
 */
export async function deleteMalAnimeStatus(animeIdOrMalId: string): Promise<void> {
    let malId: number;
    try {
        malId = await resolveMalAnimeId(animeIdOrMalId);
    } catch {
        return; // can't identify the entry: nothing to delete
    }
    const res = await malFetch(`/anime/${malId}/my_list_status`, { method: 'DELETE' });
    if (!res.ok && res.status !== 404) console.error('[MAL Sync] Delete failed', res.status);
}

// ── Manga ────────────────────────────────────────────────────────────────────

/**
 * Fetches the user's entire manga list (manga and novels) from MyAnimeList.
 */
export async function fetchMalMangaList() {
    return fetchAllMalPages('/users/@me/mangalist?fields=list_status,media_type&limit=1000&nsfw=true');
}

/**
 * Updates the status of a manga on the user's MyAnimeList
 * (`PATCH /manga/{id}/my_list_status`).
 */
export async function updateMalMangaStatus(
    mangaIdOrMalId: string | number,
    status: string,
    score?: number,
    numReadChapters?: number
) {
    const malId = await resolveMalMangaId(mangaIdOrMalId);
    const res = await malFetch(`/manga/${malId}/my_list_status`, {
        method: 'PATCH',
        form: {
            status: mapTatakaiMangaStatusToMal(status),
            score: malScore(score),
            num_chapters_read: numReadChapters !== undefined ? Math.max(0, Math.trunc(numReadChapters)) : undefined,
        },
    });
    await failIfNotOk(res, 'MAL manga sync');
    return res.json();
}

/**
 * Removes a manga from the user's MyAnimeList library.
 */
export async function deleteMalMangaStatus(mangaIdOrMalId: string): Promise<void> {
    const malId = await resolveMalMangaId(mangaIdOrMalId);
    const res = await malFetch(`/manga/${malId}/my_list_status`, { method: 'DELETE' });
    if (!res.ok && res.status !== 404) console.error('[MAL Manga Sync] Delete failed', res.status);
}

// ── Account ──────────────────────────────────────────────────────────────────

/**
 * Checks if the user is linked to MAL.
 */
export async function getMalStatus(profile: any) {
    // The token itself is private; mal_user_id on the (public) profile is the linked signal.
    return !!(profile?.mal_user_id);
}

/**
 * Disconnects the user's MyAnimeList account by clearing tokens.
 */
export async function disconnectMal(_userId?: string): Promise<void> {
    await clearMalCredentials();
}

/** Clear the stored login for the signed-in user (used after an unrecoverable 401). */
export const clearStoredMalLogin = clearMalCredentials;

export { MAL_API_URL };
