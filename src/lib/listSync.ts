/**
 * Client-side list sync between Tatakai's own lists (Supabase tables `watchlist`,
 * `watch_history`, `manga_readlist`) and the user's AniList / MyAnimeList lists.
 *
 * Replaces TatakaiAPI's `/sync/preview` + `/sync/apply`: the remote list is read
 * straight from AniList (GraphQL, implicit-grant token) or MAL (REST v2, PKCE
 * token), diffed against the local list, shown to the user for approval, and the
 * approved items are written to whichever side they point at.
 *
 * Pure parts (status maps, `diffLists`, `applySyncActions`) take their I/O as
 * parameters so they are unit tested offline.
 */

import {
  deleteAniListEntry,
  fetchAniListList,
  mapAniListAnimeStatusToTatakai,
  mapAniListMangaStatusToTatakai,
  mapTatakaiMangaStatusToAniList,
  mapTatakaiStatusToAniList,
  updateAniListAnimeStatus,
  updateAniListMangaStatus,
  type AniListListEntry,
} from '@/lib/externalIntegrations';
import {
  fetchMalMangaList,
  fetchMalUserList,
  mapMalMangaStatusToTatakai,
  mapMalStatusToTatakai,
  mapTatakaiMangaStatusToMal,
  mapTatakaiStatusToMal,
  updateMalAnimeStatus,
  updateMalMangaStatus,
} from '@/lib/mal';
import { AniListAuthExpiredError, AniListUnavailableError } from '@/lib/anilist/auth';
import { MalAuthExpiredError } from '@/lib/mal/auth';
import { MangaBakaUnavailableError } from '@/lib/mangabaka';

export type Integration = 'mal' | 'anilist' | 'mangabaka';
export type MediaType = 'anime' | 'manga' | 'novel';

export interface SyncProposalItem {
  id: string;
  mediaId: string | number;
  localMediaIds?: string[];
  malId?: number | null;
  anilistId?: number | null;
  mangabakaId?: number | null;
  title: string;
  poster: string | null;
  type: MediaType;
  direction: 'import' | 'export' | 'conflict';
  /** For conflicts: which side wins when the item is applied. */
  resolve?: 'import' | 'export';
  localStatus?: string | null;
  localProgress?: number | null;
  remoteStatus?: string | null;
  remoteProgress?: number | null;
  localVolumes?: number | null;
  remoteVolumes?: number | null;
  actionText: string;
}

// ── Status maps (Tatakai <-> AniList <-> MAL) ────────────────────────────────

/** Tatakai watchlist / readlist status -> AniList MediaListStatus. */
export function toAniListStatus(local: string, type: MediaType): string {
  return type === 'anime' ? mapTatakaiStatusToAniList(local) : mapTatakaiMangaStatusToAniList(local);
}

/** AniList MediaListStatus -> Tatakai status. */
export function fromAniListStatus(remote: string, type: MediaType): string {
  return type === 'anime' ? mapAniListAnimeStatusToTatakai(remote) : mapAniListMangaStatusToTatakai(remote);
}

/** Tatakai status -> MAL list status. */
export function toMalStatus(local: string, type: MediaType): string {
  return type === 'anime' ? mapTatakaiStatusToMal(local) : mapTatakaiMangaStatusToMal(local);
}

/** MAL list status -> Tatakai status. */
export function fromMalStatus(remote: string, type: MediaType): string {
  return type === 'anime' ? mapMalStatusToTatakai(remote) : mapMalMangaStatusToTatakai(remote);
}

// ── Normalized entries ───────────────────────────────────────────────────────

export interface LocalEntry {
  /** anime_id / manga_id in the local table. */
  key: string;
  type: MediaType;
  title: string;
  poster: string | null;
  /** Tatakai status vocabulary. */
  status: string;
  /** Episodes watched / chapters read; null when unknown (never overwrites a remote value). */
  progress: number | null;
  malId: number | null;
  anilistId: number | null;
  updatedAt?: string | null;
}

export interface RemoteEntry {
  type: MediaType;
  malId: number | null;
  anilistId: number | null;
  title: string;
  poster: string | null;
  /** Tatakai status vocabulary (mapped from the raw one). */
  status: string;
  /** The provider's own status string, for display. */
  remoteStatus: string;
  progress: number | null;
  volumes: number | null;
  updatedAt?: string | null;
}

/** Numeric ids implied by a local key (`anilist:12`, `12`, `mal:7`) and columns. */
export function localIds(entry: Pick<LocalEntry, 'key' | 'malId' | 'anilistId'>): { malId: number | null; anilistId: number | null } {
  let anilistId = entry.anilistId;
  let malId = entry.malId;
  const anilistKey = /^(?:anilist:)?(\d+)$/i.exec(entry.key);
  if (!anilistId && anilistKey) anilistId = Number(anilistKey[1]);
  const malKey = /^mal[:-](\d+)$/i.exec(entry.key);
  if (!malId && malKey) malId = Number(malKey[1]);
  return { malId: malId ?? null, anilistId: anilistId ?? null };
}

const NOVEL_FORMATS = new Set(['NOVEL', 'LIGHT_NOVEL']);
const NOVEL_MAL_TYPES = new Set(['light_novel', 'novel']);

export function remoteFromAniList(entry: AniListListEntry, type: MediaType): RemoteEntry | null {
  if (!entry?.media) return null;
  const m = entry.media;
  return {
    type,
    malId: m.idMal ?? null,
    anilistId: m.id ?? entry.mediaId ?? null,
    title: m.title?.english || m.title?.romaji || m.title?.native || '',
    poster: m.coverImage?.large || m.coverImage?.medium || null,
    status: fromAniListStatus(entry.status, type),
    remoteStatus: String(entry.status || '').toLowerCase(),
    progress: entry.progress ?? 0,
    volumes: entry.progressVolumes ?? null,
    updatedAt: entry.updatedAt ? new Date(entry.updatedAt * 1000).toISOString() : null,
  };
}

export function remoteFromMal(row: any, type: MediaType): RemoteEntry | null {
  const node = row?.node;
  const ls = row?.list_status;
  if (!node?.id || !ls) return null;
  return {
    type,
    malId: Number(node.id),
    anilistId: null,
    title: node.title || '',
    poster: node.main_picture?.large || node.main_picture?.medium || null,
    status: fromMalStatus(ls.status, type),
    remoteStatus: String(ls.status || ''),
    progress: Number(type === 'anime' ? ls.num_episodes_watched : ls.num_chapters_read) || 0,
    volumes: type === 'anime' ? null : (Number(ls.num_volumes_read) || null),
    updatedAt: ls.updated_at || null,
  };
}

// ── Diff ─────────────────────────────────────────────────────────────────────

const LABEL: Record<Integration, string> = { mal: 'MyAnimeList', anilist: 'AniList', mangabaka: 'MangaBaka' };
const unit = (type: MediaType) => (type === 'anime' ? 'episode' : 'chapter');

function describe(status: string | null | undefined, progress: number | null | undefined, type: MediaType): string {
  const parts = [status ? status.replace(/_/g, ' ') : ''];
  if (progress != null) parts.push(`${unit(type)} ${progress}`);
  return parts.filter(Boolean).join(', ');
}

function targetId(integration: Integration, ids: { malId: number | null; anilistId: number | null }): number | null {
  return integration === 'anilist' ? ids.anilistId : integration === 'mal' ? ids.malId : null;
}

/**
 * Compare the local list with the remote one. Rules:
 *  - remote only                      -> import
 *  - local only (and addressable)     -> export
 *  - same title, only progress differs-> the higher progress wins (import/export)
 *  - same title, status differs       -> conflict; the more recently updated side
 *                                        wins on apply (local wins when unknown)
 * Local entries with unknown progress (null) never overwrite a remote value.
 */
export function diffLists(integration: Integration, local: LocalEntry[], remote: RemoteEntry[], type: MediaType): SyncProposalItem[] {
  const items: SyncProposalItem[] = [];
  const byAnilist = new Map<number, LocalEntry>();
  const byMal = new Map<number, LocalEntry>();
  for (const l of local) {
    const ids = localIds(l);
    if (ids.anilistId) byAnilist.set(ids.anilistId, l);
    if (ids.malId) byMal.set(ids.malId, l);
  }
  const matched = new Set<LocalEntry>();
  const label = LABEL[integration];

  for (const r of remote) {
    const l = (r.anilistId ? byAnilist.get(r.anilistId) : undefined) ?? (r.malId ? byMal.get(r.malId) : undefined);
    const base = {
      mediaId: r.anilistId ?? r.malId ?? '',
      malId: r.malId,
      anilistId: r.anilistId,
      title: r.title || l?.title || '',
      poster: r.poster ?? l?.poster ?? null,
      type,
      remoteStatus: r.status,
      remoteProgress: r.progress,
      remoteVolumes: r.volumes,
    };
    const idKey = `${integration}:${type}:${r.anilistId ?? 'm' + r.malId}`;
    if (!l) {
      items.push({ ...base, id: idKey, direction: 'import', actionText: `Add to your Tatakai list from ${label}: ${describe(r.status, r.progress, type)}` });
      continue;
    }
    matched.add(l);
    const statusDiff = l.status !== r.status;
    const bothProgress = l.progress != null && r.progress != null;
    const progressDiff = bothProgress && l.progress !== r.progress;
    if (!statusDiff && !progressDiff) continue;
    const common = { ...base, id: idKey, localMediaIds: [l.key], mediaId: l.key, localStatus: l.status, localProgress: l.progress };
    if (!statusDiff) {
      const importIt = (r.progress ?? 0) > (l.progress ?? 0);
      items.push({
        ...common,
        direction: importIt ? 'import' : 'export',
        actionText: importIt
          ? `${label} is ahead: update Tatakai to ${unit(type)} ${r.progress}`
          : `Tatakai is ahead: update ${label} to ${unit(type)} ${l.progress}`,
      });
      continue;
    }
    const remoteNewer = Boolean(r.updatedAt && l.updatedAt && Date.parse(r.updatedAt) > Date.parse(l.updatedAt));
    items.push({
      ...common,
      direction: 'conflict',
      resolve: remoteNewer ? 'import' : 'export',
      actionText: remoteNewer
        ? `Status differs; ${label} was updated more recently, so it will overwrite Tatakai`
        : `Status differs; Tatakai will overwrite ${label}`,
    });
  }

  for (const l of local) {
    if (matched.has(l)) continue;
    const ids = localIds(l);
    if (!targetId(integration, ids)) continue; // can't address this title on the remote side
    items.push({
      id: `${integration}:${type}:l:${l.key}`,
      mediaId: l.key,
      localMediaIds: [l.key],
      malId: ids.malId,
      anilistId: ids.anilistId,
      title: l.title,
      poster: l.poster,
      type,
      direction: 'export',
      localStatus: l.status,
      localProgress: l.progress,
      actionText: `Add to ${label}: ${describe(l.status, l.progress, type)}`,
    });
  }
  return items;
}

// ── Apply ────────────────────────────────────────────────────────────────────

export interface SyncIO {
  writeLocal(item: SyncProposalItem): Promise<void>;
  writeRemote(integration: Integration, item: SyncProposalItem, status: string, progress: number | undefined): Promise<void>;
}

export interface ApplyResult {
  applied: number;
  failed: Array<{ id: string; error: string }>;
}

/** Errors that mean "stop, every further call will fail the same way". */
function isFatal(err: unknown): boolean {
  return err instanceof AniListAuthExpiredError || err instanceof AniListUnavailableError || err instanceof MalAuthExpiredError || err instanceof MangaBakaUnavailableError;
}

export async function applySyncActions(integration: Integration, actions: SyncProposalItem[], io: SyncIO): Promise<ApplyResult> {
  const result: ApplyResult = { applied: 0, failed: [] };
  for (const item of actions) {
    const effective = item.direction === 'conflict' ? (item.resolve ?? 'export') : item.direction;
    try {
      if (effective === 'import') {
        await io.writeLocal(item);
      } else {
        const status = toRemoteStatus(integration, item);
        await io.writeRemote(integration, item, status, exportProgress(item));
      }
      result.applied++;
    } catch (err) {
      if (isFatal(err)) throw err;
      result.failed.push({ id: item.id, error: err instanceof Error ? err.message : String(err) });
    }
  }
  return result;
}

/**
 * Progress to push to the remote list: never lower than what the remote already
 * has (a conflict can export a status while the remote is ahead on progress), and
 * omitted entirely when the local side doesn't know its progress.
 */
export function exportProgress(item: Pick<SyncProposalItem, 'localProgress' | 'remoteProgress'>): number | undefined {
  if (item.localProgress == null) return undefined;
  return Math.max(item.localProgress, item.remoteProgress ?? 0);
}

function toRemoteStatus(integration: Integration, item: SyncProposalItem): string {
  const local = item.localStatus || (item.type === 'anime' ? 'plan_to_watch' : 'plan_to_read');
  return integration === 'anilist' ? toAniListStatus(local, item.type) : toMalStatus(local, item.type);
}

// ── Real I/O (Supabase + providers) ──────────────────────────────────────────

async function db() {
  const { supabase } = await import('@/integrations/supabase/client');
  const { data: { session } } = await supabase.auth.getSession();
  if (!session?.user) throw new Error('Not authenticated');
  return { supabase: supabase as any, userId: session.user.id };
}

async function selectAll(build: (from: number, to: number) => PromiseLike<{ data: any[] | null; error: any }>): Promise<any[]> {
  const rows: any[] = [];
  for (let from = 0; from < 20000; from += 1000) {
    const { data, error } = await build(from, from + 999);
    if (error) throw error;
    rows.push(...(data || []));
    if (!data || data.length < 1000) break;
  }
  return rows;
}

export async function loadLocalEntries(type: MediaType): Promise<LocalEntry[]> {
  const { supabase, userId } = await db();
  if (type === 'anime') {
    const list = await selectAll((a, b) => supabase.from('watchlist').select('*').eq('user_id', userId).range(a, b));
    const history = await selectAll((a, b) => supabase.from('watch_history').select('anime_id, episode_number, completed').eq('user_id', userId).range(a, b));
    const watched = new Map<string, number>();
    for (const h of history) {
      if (h.completed) watched.set(h.anime_id, Math.max(watched.get(h.anime_id) ?? 0, Number(h.episode_number) || 0));
    }
    return list.map((r): LocalEntry => ({
      key: r.anime_id, type, title: r.anime_name, poster: r.anime_poster ?? null,
      status: r.status || 'plan_to_watch',
      progress: watched.has(r.anime_id) ? watched.get(r.anime_id)! : null,
      malId: r.mal_id ?? null, anilistId: r.anilist_id ?? null, updatedAt: r.updated_at ?? null,
    }));
  }
  const rows = await selectAll((a, b) => supabase.from('manga_readlist').select('*').eq('user_id', userId).range(a, b));
  return rows
    .filter((r) => (String(r.format || 'manga').toLowerCase() === 'novel') === (type === 'novel'))
    .map((r): LocalEntry => ({
      key: r.manga_id, type, title: r.manga_title, poster: r.manga_poster ?? null,
      status: r.status || 'plan_to_read',
      progress: r.last_chapter_number != null ? Math.trunc(Number(r.last_chapter_number)) : null,
      malId: r.mal_id ?? null, anilistId: r.anilist_id ?? null, updatedAt: r.updated_at ?? null,
    }));
}

async function loadAniListLogin(): Promise<{ token: string; userId: number }> {
  // Token from the owner-only user_integration_tokens table, account id from profiles.
  const { loadAniListCredentials } = await import('@/lib/integrationTokens');
  const login = await loadAniListCredentials();
  if (!login) throw new AniListAuthExpiredError();
  return login;
}

/** Remote lists for one integration and media type, normalized. */
export async function fetchRemoteEntries(integration: Integration, type: MediaType): Promise<RemoteEntry[]> {
  if (integration === 'mangabaka') throw new MangaBakaUnavailableError();
  if (integration === 'anilist') {
    const login = await loadAniListLogin();
    const rows = await fetchAniListList(login.token, login.userId, type === 'anime' ? 'ANIME' : 'MANGA');
    return rows
      .filter((r) => type === 'anime' || NOVEL_FORMATS.has(String(r.media?.format || '').toUpperCase()) === (type === 'novel'))
      .map((r) => remoteFromAniList(r, type))
      .filter((r): r is RemoteEntry => r !== null);
  }
  if (type === 'anime') {
    return (await fetchMalUserList()).map((r) => remoteFromMal(r, type)).filter((r): r is RemoteEntry => r !== null);
  }
  const rows = await fetchMalMangaList();
  return rows
    .filter((r: any) => NOVEL_MAL_TYPES.has(String(r?.node?.media_type || '')) === (type === 'novel'))
    .map((r: any) => remoteFromMal(r, type))
    .filter((r: RemoteEntry | null): r is RemoteEntry => r !== null);
}

/** malIds -> anilistIds (unknown ids are absent from the map). */
export type MalToAniListResolver = (type: MediaType, malIds: number[]) => Promise<Map<number, number>>;

const defaultResolver: MalToAniListResolver = async (type, malIds) => {
  const { ensureAnilistIds } = await import('@/lib/metadata');
  const items = malIds.map((malId) => ({
    anilistId: 0, malId, titleRomaji: '', synonyms: [], genres: [], tags: [], studios: [],
    isAdult: false, source_api: 'mal' as const, fetchedAt: 0,
  }));
  const resolved = await ensureAnilistIds(type === 'anime' ? 'ANIME' : 'MANGA', items);
  return new Map(resolved.filter((m) => m.malId).map((m) => [m.malId as number, m.anilistId]));
};

/**
 * MAL entries only carry a MAL id, so a local row whose `mal_id` column is NULL
 * would never match and the title would be imported a second time. Resolve the
 * remote MAL ids to AniList ids (cached, one bulk lookup per list) so
 * `diffLists` can match on the AniList id first. Only ids no local row already
 * claims by MAL id are looked up, and a failed lookup just leaves them unresolved.
 */
export async function attachAniListIds(
  remote: RemoteEntry[],
  local: LocalEntry[],
  type: MediaType,
  resolve: MalToAniListResolver = defaultResolver,
): Promise<RemoteEntry[]> {
  const localMal = new Set<number>();
  let localLacksMal = false;
  for (const l of local) {
    const ids = localIds(l);
    if (ids.malId) localMal.add(ids.malId);
    else localLacksMal = true;
  }
  if (!localLacksMal) return remote;
  const need = [...new Set(remote.filter((r) => !r.anilistId && r.malId && !localMal.has(r.malId)).map((r) => r.malId as number))];
  if (!need.length) return remote;
  let map: Map<number, number>;
  try { map = await resolve(type, need); } catch { return remote; }
  return remote.map((r) => (r.anilistId || !r.malId ? r : { ...r, anilistId: map.get(r.malId) ?? null }));
}

/** Preview for one media type. */
export async function previewSync(integration: Integration, type: MediaType): Promise<SyncProposalItem[]> {
  const [local, remoteRaw] = await Promise.all([loadLocalEntries(type), fetchRemoteEntries(integration, type)]);
  const remote = integration === 'mal' ? await attachAniListIds(remoteRaw, local, type) : remoteRaw;
  return diffLists(integration, local, remote, type);
}

/** Preview for every media type the integration covers (anime + manga + novels). */
export async function previewAllSync(integration: Integration): Promise<SyncProposalItem[]> {
  if (integration === 'mangabaka') throw new MangaBakaUnavailableError();
  const out: SyncProposalItem[] = [];
  for (const type of ['anime', 'manga', 'novel'] as MediaType[]) out.push(...await previewSync(integration, type));
  return out;
}

const realIO: SyncIO = {
  async writeLocal(item) {
    const { supabase, userId } = await db();
    const now = new Date().toISOString();
    const status = item.remoteStatus || (item.type === 'anime' ? 'plan_to_watch' : 'plan_to_read');
    const existingKey = item.localMediaIds?.[0];
    if (item.type === 'anime') {
      const key = existingKey ?? (item.anilistId ? String(item.anilistId) : `mal:${item.malId}`);
      if (existingKey) {
        // Existing row: only status plus ids the item actually has. Name, poster
        // and ids already stored are never overwritten with null / other-site data.
        const patch: Record<string, unknown> = { status, updated_at: now };
        if (item.malId) patch.mal_id = item.malId;
        if (item.anilistId) patch.anilist_id = item.anilistId;
        const { error } = await supabase.from('watchlist').update(patch).eq('user_id', userId).eq('anime_id', existingKey);
        if (error) throw error;
      } else {
        const { error } = await supabase.from('watchlist').upsert({
          user_id: userId, anime_id: key, anime_name: item.title, anime_poster: item.poster, status,
          mal_id: item.malId ?? null, anilist_id: item.anilistId ?? null, updated_at: now,
        }, { onConflict: 'user_id,anime_id' });
        if (error) throw error;
      }
      // Anime progress is derived from watch_history, so an import that raises it
      // must record the episode, otherwise the same item shows up in every preview.
      const remoteEp = item.remoteProgress ?? 0;
      if (remoteEp > 0 && remoteEp > (item.localProgress ?? 0)) {
        const { error } = await supabase.from('watch_history').upsert({
          user_id: userId, anime_id: key, anime_name: item.title, anime_poster: item.poster,
          episode_id: `sync:${key}:${remoteEp}`, episode_number: remoteEp,
          progress_seconds: 0, duration_seconds: 0, completed: true, watched_at: now,
          mal_id: item.malId ?? null, anilist_id: item.anilistId ?? null,
        }, { onConflict: 'user_id,episode_id' });
        if (error) throw error;
      }
      return;
    }
    if (existingKey) {
      const patch: Record<string, unknown> = { status, updated_at: now };
      if (item.remoteProgress != null) patch.last_chapter_number = item.remoteProgress;
      const { error } = await supabase.from('manga_readlist').update(patch).eq('user_id', userId).eq('manga_id', existingKey);
      if (error) throw error;
      return;
    }
    const key = item.anilistId ? `anilist:${item.anilistId}` : `mal:${item.malId}`;
    const { error } = await supabase.from('manga_readlist').upsert({
      user_id: userId, manga_id: key, manga_title: item.title, manga_poster: item.poster, status,
      last_chapter_number: item.remoteProgress ?? null, last_page_index: 0,
      format: item.type === 'novel' ? 'novel' : 'manga',
      mal_id: item.malId ?? null, anilist_id: item.anilistId ?? null, updated_at: now,
    }, { onConflict: 'user_id,manga_id' });
    if (error) throw error;
  },

  async writeRemote(integration, item, status, progress) {
    if (integration === 'mangabaka') throw new MangaBakaUnavailableError();
    if (integration === 'anilist') {
      if (!item.anilistId) throw new Error('No AniList id for this title');
      const { token } = await loadAniListLogin();
      if (item.type === 'anime') await updateAniListAnimeStatus(token, item.anilistId, status, progress);
      else await updateAniListMangaStatus(token, item.anilistId, status, progress);
      return;
    }
    if (!item.malId) throw new Error('No MyAnimeList id for this title');
    if (item.type === 'anime') await updateMalAnimeStatus(item.malId, status, undefined, progress);
    else await updateMalMangaStatus(item.malId, status, undefined, progress);
  },
};

export function applySync(integration: Integration, actions: SyncProposalItem[]): Promise<ApplyResult> {
  return applySyncActions(integration, actions, realIO);
}

/**
 * Push one item to every linked tracker that knows its id (the "smart auto-sync"
 * path after finishing an episode/chapter). Best effort per tracker.
 */
export async function pushSingleItem(params: {
  type: 'anime' | 'manga';
  malId?: number | null;
  anilistId?: number | null;
  /** Tatakai status vocabulary. */
  status: string;
  progress: number;
  score?: number;
}): Promise<void> {
  const { loadIntegrationTokens } = await import('@/lib/integrationTokens');
  const data = await loadIntegrationTokens();
  const jobs: Array<Promise<unknown>> = [];
  if (data?.anilist_access_token && params.anilistId) {
    jobs.push(params.type === 'anime'
      ? updateAniListAnimeStatus(data.anilist_access_token, params.anilistId, params.status, params.progress, params.score ? params.score * 10 : undefined)
      : updateAniListMangaStatus(data.anilist_access_token, params.anilistId, params.status, params.progress, params.score ? params.score * 10 : undefined));
  }
  if (data?.mal_access_token && params.malId) {
    jobs.push(params.type === 'anime'
      ? updateMalAnimeStatus(params.malId, params.status, params.score, params.progress)
      : updateMalMangaStatus(params.malId, params.status, params.score, params.progress));
  }
  await Promise.allSettled(jobs);
}

export { deleteAniListEntry };

/** The Supabase/provider-backed I/O used by the app (exported for tests). */
export const defaultSyncIO: SyncIO = realIO;
