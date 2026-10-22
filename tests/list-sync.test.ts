import { expect, test } from 'bun:test';
import {
  applySyncActions,
  diffLists,
  fromAniListStatus,
  fromMalStatus,
  localIds,
  remoteFromAniList,
  remoteFromMal,
  toAniListStatus,
  toMalStatus,
  type LocalEntry,
  type RemoteEntry,
  type SyncIO,
  type SyncProposalItem,
} from '../src/lib/listSync';
import { AniListUnavailableError } from '../src/lib/anilist/auth';
import { MalAuthExpiredError } from '../src/lib/mal/auth';

const ANIME = [
  // tatakai, anilist, mal
  ['watching', 'CURRENT', 'watching'],
  ['plan_to_watch', 'PLANNING', 'plan_to_watch'],
  ['completed', 'COMPLETED', 'completed'],
  ['dropped', 'DROPPED', 'dropped'],
  ['on_hold', 'PAUSED', 'on_hold'],
] as const;

const MANGA = [
  ['reading', 'CURRENT', 'reading'],
  ['plan_to_read', 'PLANNING', 'plan_to_read'],
  ['completed', 'COMPLETED', 'completed'],
  ['dropped', 'DROPPED', 'dropped'],
  ['on_hold', 'PAUSED', 'on_hold'],
] as const;

test('anime statuses map both ways between Tatakai, AniList and MAL', () => {
  for (const [local, anilist, mal] of ANIME) {
    expect(toAniListStatus(local, 'anime')).toBe(anilist);
    expect(fromAniListStatus(anilist, 'anime')).toBe(local);
    expect(toMalStatus(local, 'anime')).toBe(mal);
    expect(fromMalStatus(mal, 'anime')).toBe(local);
  }
  // AniList-only REPEATING collapses to watching; unknown values fall back to planning
  expect(fromAniListStatus('REPEATING', 'anime')).toBe('watching');
  expect(fromAniListStatus('nonsense', 'anime')).toBe('plan_to_watch');
  expect(toMalStatus('nonsense', 'anime')).toBe('plan_to_watch');
});

test('manga and novel statuses map both ways', () => {
  for (const [local, anilist, mal] of MANGA) {
    for (const type of ['manga', 'novel'] as const) {
      expect(toAniListStatus(local, type)).toBe(anilist);
      expect(fromAniListStatus(anilist, type)).toBe(local);
      expect(toMalStatus(local, type)).toBe(mal);
      expect(fromMalStatus(mal, type)).toBe(local);
    }
  }
  expect(fromAniListStatus('REPEATING', 'manga')).toBe('reading');
  expect(fromMalStatus('nonsense', 'manga')).toBe('plan_to_read');
});

test('either status vocabulary is accepted when writing to MAL', () => {
  expect(toMalStatus('CURRENT', 'anime')).toBe('watching');
  expect(toMalStatus('PLANNING', 'anime')).toBe('plan_to_watch');
  expect(toMalStatus('PAUSED', 'manga')).toBe('on_hold');
});

const local = (key: string, status: string, progress: number | null, extra: Partial<LocalEntry> = {}): LocalEntry => ({
  key, type: 'anime', title: `T ${key}`, poster: null, status, progress, malId: null, anilistId: null, ...extra,
});
const remote = (anilistId: number | null, malId: number | null, status: string, progress: number | null, extra: Partial<RemoteEntry> = {}): RemoteEntry => ({
  type: 'anime', anilistId, malId, title: `R ${anilistId ?? malId}`, poster: null, status, remoteStatus: status, progress, volumes: null, ...extra,
});

test('local ids come from columns or from the local key', () => {
  expect(localIds({ key: '154587', malId: null, anilistId: null })).toEqual({ malId: null, anilistId: 154587 });
  expect(localIds({ key: 'anilist:30002', malId: null, anilistId: null })).toEqual({ malId: null, anilistId: 30002 });
  expect(localIds({ key: 'mal:52991', malId: null, anilistId: null })).toEqual({ malId: 52991, anilistId: null });
  expect(localIds({ key: 'slug-x', malId: 5, anilistId: 6 })).toEqual({ malId: 5, anilistId: 6 });
});

test('diff: remote-only imports, local-only exports, progress-only follows the higher side, status differences conflict', () => {
  const items = diffLists('anilist', [
    local('2', 'watching', 3, { anilistId: 2 }),                  // remote ahead -> import
    local('3', 'watching', 9, { anilistId: 3 }),                  // local ahead  -> export
    local('4', 'completed', 12, { anilistId: 4 }),                // status differs -> conflict
    local('5', 'plan_to_watch', null, { anilistId: 5 }),          // local-only -> export
    local('slug', 'watching', 1),                                 // no AniList id -> skipped
    local('6', 'watching', 4, { anilistId: 6 }),                  // identical -> nothing
  ], [
    remote(1, null, 'watching', 2),                               // remote-only -> import
    remote(2, null, 'watching', 8),
    remote(3, null, 'watching', 5),
    remote(4, null, 'dropped', 12),
    remote(6, null, 'watching', 4),
  ], 'anime');

  const by = Object.fromEntries(items.map((i) => [String(i.anilistId), i]));
  expect(items).toHaveLength(5);
  expect(by['1']).toMatchObject({ direction: 'import', remoteStatus: 'watching', remoteProgress: 2 });
  expect(by['2']).toMatchObject({ direction: 'import', localProgress: 3, remoteProgress: 8, localMediaIds: ['2'] });
  expect(by['3']).toMatchObject({ direction: 'export', localProgress: 9, remoteProgress: 5 });
  expect(by['4']).toMatchObject({ direction: 'conflict', resolve: 'export', localStatus: 'completed', remoteStatus: 'dropped' });
  expect(by['5']).toMatchObject({ direction: 'export', localStatus: 'plan_to_watch' });
});

test('diff: unknown local progress never overwrites the remote one', () => {
  const items = diffLists('mal', [local('mal:10', 'watching', null)], [remote(null, 10, 'watching', 7)], 'anime');
  expect(items).toEqual([]);
});

test('diff: the more recently updated side wins a status conflict', () => {
  const l = local('1', 'completed', 12, { anilistId: 1, updatedAt: '2026-01-01T00:00:00Z' });
  const newerRemote = remote(1, null, 'dropped', 12, { updatedAt: '2026-06-01T00:00:00Z' });
  const olderRemote = remote(1, null, 'dropped', 12, { updatedAt: '2025-06-01T00:00:00Z' });
  expect(diffLists('anilist', [l], [newerRemote], 'anime')[0]).toMatchObject({ direction: 'conflict', resolve: 'import' });
  expect(diffLists('anilist', [l], [olderRemote], 'anime')[0]).toMatchObject({ direction: 'conflict', resolve: 'export' });
});

test('diff: MAL matches on mal id and AniList entries also match through idMal', () => {
  const l = local('x', 'watching', 1, { malId: 52991 });
  expect(diffLists('mal', [l], [remote(null, 52991, 'watching', 4)], 'anime')[0]).toMatchObject({ direction: 'import', malId: 52991 });
  expect(diffLists('anilist', [l], [remote(154587, 52991, 'watching', 4)], 'anime')[0]).toMatchObject({ direction: 'import' });
});

test('remote entries normalize from AniList and MAL payloads', () => {
  const al = remoteFromAniList({
    id: 1, mediaId: 154587, status: 'PAUSED', progress: 6, score: 80, updatedAt: 1_700_000_000,
    media: { id: 154587, idMal: 52991, title: { romaji: 'Sousou no Frieren', english: 'Frieren' }, coverImage: { large: 'L', medium: 'M' } },
  }, 'anime');
  expect(al).toMatchObject({ anilistId: 154587, malId: 52991, title: 'Frieren', status: 'on_hold', remoteStatus: 'paused', progress: 6, poster: 'L' });
  expect(al?.updatedAt).toBe(new Date(1_700_000_000_000).toISOString());

  const mal = remoteFromMal({ node: { id: 52991, title: 'Frieren', main_picture: { medium: 'M' } }, list_status: { status: 'plan_to_watch', num_episodes_watched: 0, updated_at: '2026-01-01T00:00:00+00:00' } }, 'anime');
  expect(mal).toMatchObject({ malId: 52991, status: 'plan_to_watch', progress: 0, poster: 'M' });
  const manga = remoteFromMal({ node: { id: 2, title: 'Berserk' }, list_status: { status: 'reading', num_chapters_read: 100, num_volumes_read: 12 } }, 'manga');
  expect(manga).toMatchObject({ status: 'reading', progress: 100, volumes: 12 });
  expect(remoteFromMal({ node: { id: 1 } }, 'anime')).toBeNull();
});

function recorder(): { io: SyncIO; local: string[]; remote: Array<[string, string, number | undefined]> } {
  const rec = { local: [] as string[], remote: [] as Array<[string, string, number | undefined]> };
  return {
    ...rec,
    local: rec.local,
    remote: rec.remote,
    io: {
      writeLocal: async (item) => { rec.local.push(item.id); },
      writeRemote: async (_integration, item, status, progress) => { rec.remote.push([item.id, status, progress]); },
    },
  };
}

const item = (id: string, direction: SyncProposalItem['direction'], extra: Partial<SyncProposalItem> = {}): SyncProposalItem => ({
  id, mediaId: id, title: id, poster: null, type: 'anime', direction, localStatus: 'watching', localProgress: 3, actionText: '', ...extra,
});

test('apply: imports write locally, exports write the remote list with the mapped status, conflicts follow their resolution', async () => {
  const r = recorder();
  const result = await applySyncActions('anilist', [
    item('a', 'import'),
    item('b', 'export'),
    item('c', 'conflict', { resolve: 'import' }),
    item('d', 'conflict', { resolve: 'export', localStatus: 'on_hold' }),
    item('e', 'conflict'),
  ], r.io);
  expect(result).toEqual({ applied: 5, failed: [] });
  expect(r.local).toEqual(['a', 'c']);
  expect(r.remote).toEqual([['b', 'CURRENT', 3], ['d', 'PAUSED', 3], ['e', 'CURRENT', 3]]);

  const mal = recorder();
  await applySyncActions('mal', [item('m', 'export', { localStatus: 'plan_to_watch', localProgress: null })], mal.io);
  expect(mal.remote).toEqual([['m', 'plan_to_watch', undefined]]);
});

test('apply: one failing item is reported and the rest still run', async () => {
  const io: SyncIO = {
    writeLocal: async () => undefined,
    writeRemote: async (_i, it) => { if (it.id === 'bad') throw new Error('MAL 404'); },
  };
  const result = await applySyncActions('mal', [item('bad', 'export'), item('ok', 'export')], io);
  expect(result.applied).toBe(1);
  expect(result.failed).toEqual([{ id: 'bad', error: 'MAL 404' }]);
});

test('apply: AniList unavailable or an expired login stops the whole run instead of looping', async () => {
  let attempts = 0;
  const unavailable: SyncIO = { writeLocal: async () => undefined, writeRemote: async () => { attempts++; throw new AniListUnavailableError(403); } };
  await expect(applySyncActions('anilist', [item('1', 'export'), item('2', 'export'), item('3', 'export')], unavailable))
    .rejects.toBeInstanceOf(AniListUnavailableError);
  expect(attempts).toBe(1);

  const expired: SyncIO = { writeLocal: async () => undefined, writeRemote: async () => { throw new MalAuthExpiredError(); } };
  await expect(applySyncActions('mal', [item('1', 'export')], expired)).rejects.toBeInstanceOf(MalAuthExpiredError);
});
