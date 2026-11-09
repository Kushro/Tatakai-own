import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Validates contracts/parity/checklist.json (docs/redesign/14-parity-plan.md §4, contracts/parity/README.md).

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..');
const list = JSON.parse(readFileSync(join(repo, 'contracts', 'parity', 'checklist.json'), 'utf8'));

const WAVES = ['A', 'B'];
const DESKTOP_OWNERS = { A: new Set(['a', 'c', 'd', 'e', 'f', 'g', 'i']), B: new Set(['b', 'h', 'j', 'k', 'l', 'm', 'n']) };
const ANDROID_OWNERS = { A: new Set(['o', 'p', 'q']), B: new Set(['r', 's', 't']) };
const STATUSES = new Set(['todo', 'baseline', 'out', 'done', 'verified', 'stub', 'missing']);
const AUDIT = new Set(['Falta', 'Stub', 'Parcial', 'Paridad', 'Next mejor', 'Fuera de plan', 'Bug']);
const PLATFORMS = new Set(['desktop', 'android']);
const REQUIRED = ['id', 'wave', 'area', 'android', 'title', 'classic', 'next', 'accept', 'status'];
// `found` is written only by the auditor (F4): the Next path where it found (or looked for) the feature.
const OPTIONAL = ['audit', 'platform', 'note', 'found'];
const AUDITED = new Set(['verified', 'stub', 'missing']);
// Planned work: still to build (`todo`), built by a wave and merged by its integrator (`done`), or audited (F4).
// The plan assertions below accept any of these, so integrating a wave does not break them.
const PLANNED = new Set(['todo', 'done', ...AUDITED]);
const ID_RE = /^[a-z0-9]+(-[a-z0-9]+)*(\.[a-z0-9]+(-[a-z0-9]+)*)+$/;
// Plan §0.1.11 (2026-11-09): Android torrents + debrid moved from r (wave B) to q (wave A).
const TORRENT_ID_RE = /^(torrent\.|sources\.debrid\.|player\.torrent)/;

test('parity checklist: shape, unique ids and allowed values', () => {
  assert.ok(Array.isArray(list) && list.length >= 300, `expected a few hundred entries, got ${list.length}`);
  const ids = new Set();
  for (const x of list) {
    const ctx = `entry ${x?.id ?? JSON.stringify(x).slice(0, 60)}`;
    for (const k of REQUIRED) assert.ok(k in x, `${ctx}: missing field ${k}`);
    for (const k of Object.keys(x)) assert.ok(REQUIRED.includes(k) || OPTIONAL.includes(k), `${ctx}: unknown field ${k}`);
    assert.match(x.id, ID_RE, `${ctx}: id must be <area>.<feature>[.<detail>] in kebab-case`);
    assert.ok(!ids.has(x.id), `duplicate id ${x.id}`);
    ids.add(x.id);
    assert.ok(WAVES.includes(x.wave), `${ctx}: wave must be A or B`);
    assert.ok(typeof x.title === 'string' && x.title.length > 0, `${ctx}: empty title`);
    assert.ok(typeof x.classic === 'string' && x.classic.length > 0, `${ctx}: empty classic path`);
    assert.ok(x.next === null || (typeof x.next === 'string' && x.next.length > 0), `${ctx}: next must be a path or null`);
    assert.ok(typeof x.accept === 'string' && x.accept.length >= 10, `${ctx}: accept must be one observable sentence`);
    assert.ok(STATUSES.has(x.status), `${ctx}: status ${x.status} not allowed`);
    if ('audit' in x) assert.ok(AUDIT.has(x.audit), `${ctx}: audit ${x.audit} not allowed`);
    if ('platform' in x) assert.ok(PLATFORMS.has(x.platform), `${ctx}: platform ${x.platform} not allowed`);
    if ('note' in x) assert.ok(typeof x.note === 'string' && x.note.length > 0, `${ctx}: empty note`);
    if ('found' in x) {
      assert.ok(typeof x.found === 'string' && x.found.length > 0 && !x.found.startsWith('/'), `${ctx}: found must be a non-empty repo-relative path`);
      assert.ok(AUDITED.has(x.status), `${ctx}: found is set by the auditor, so status must be verified/stub/missing (got ${x.status})`);
    }
  }
});

test('parity checklist: owner letters match the wave; android owner never from an earlier wave', () => {
  for (const x of list) {
    assert.ok(DESKTOP_OWNERS[x.wave].has(x.area), `${x.id}: desktop owner ${x.area} is not a wave ${x.wave} owner`);
    if (x.android !== null) {
      const androidWave = WAVES.find((w) => ANDROID_OWNERS[w].has(x.android));
      assert.ok(androidWave, `${x.id}: android owner ${x.android} is not a valid letter o..t`);
      assert.ok(WAVES.indexOf(androidWave) >= WAVES.indexOf(x.wave), `${x.id}: android owner ${x.android} (wave ${androidWave}) precedes desktop wave ${x.wave}`);
    }
    if (x.platform === 'desktop') assert.equal(x.android, null, `${x.id}: desktop-only entry must have android null`);
    if (x.platform === 'android') assert.notEqual(x.android, null, `${x.id}: android-only entry needs an android owner`);
    // wave A torrents/debrid: desktop i, Android q (never r, which is wave B)
    if (x.wave === 'A' && TORRENT_ID_RE.test(x.id) && x.android !== null) {
      assert.equal(x.android, 'q', `${x.id}: wave A torrent/debrid entries belong to q on Android (plan §0.1.11), got ${x.android}`);
    }
  }
});

// `next` records where a feature lives today; the wave A contract splits player.rs, settings.rs and anilist.rs into
// directories (plan §3), so a `foo.rs` path is also accepted once it became `foo/mod.rs`.
const nextExists = (p) => existsSync(join(repo, p)) || (p.endsWith('.rs') && existsSync(join(repo, p.replace(/\.rs$/, '/mod.rs'))));

test('parity checklist: every classic and next path exists; out entries carry a note', () => {
  for (const x of list) {
    assert.ok(existsSync(join(repo, x.classic)), `${x.id}: classic path does not exist: ${x.classic}`);
    if (x.next !== null) assert.ok(nextExists(x.next), `${x.id}: next path does not exist: ${x.next}`);
    if (x.status === 'out') assert.ok(x.note, `${x.id}: an "out" entry must say why in note`);
  }
});

test('parity checklist: coverage of the plan (every owner has work, both audit bugs and the audit stubs are listed)', () => {
  const todoByArea = {};
  for (const x of list) if (PLANNED.has(x.status)) todoByArea[x.area] = (todoByArea[x.area] ?? 0) + 1;
  for (const w of WAVES) for (const o of DESKTOP_OWNERS[w]) assert.ok((todoByArea[o] ?? 0) > 0, `owner ${o} has no planned entries`);
  const byId = new Map(list.map((x) => [x.id, x]));
  for (const id of ['player.subs.preferred-lang', 'sources.remembered-vs-prefs']) {
    assert.ok(byId.has(id), `missing audit bug ${id}`);
    assert.equal(byId.get(id).audit, 'Bug');
  }
  // audit "Stub" rows
  for (const id of ['search.filters.genre', 'search.in-extension', 'search.see-all-feed', 'ext.custom-providers.home-rows', 'details.related',
    'details.list-button-mount', 'link.send-to-device', 'player.screenshot', 'system.clear-cache', 'system.open-folder', 'system.export-history',
    'system.open-logs', 'system.update-check', 'system.licenses']) {
    assert.ok(byId.has(id), `missing audit stub ${id}`);
    assert.ok(PLANNED.has(byId.get(id).status), `${id} must be planned work (todo, done or audited), got ${byId.get(id).status}`);
  }
  // plan §7 exclusions are explicit
  for (const id of ['manga.yomi', 'social.wrapped', 'social.community.feed', 'system.admin', 'system.tv-mode', 'library.playlists', 'library.tierlists',
    'ext.store', 'system.dynamic-island', 'system.popups', 'ext.renderer']) {
    assert.ok(byId.has(id), `missing §7 exclusion ${id}`);
    assert.equal(byId.get(id).status, 'out', `${id} must be out`);
  }
});

test('parity checklist: plan §0.1 amendments (2026-11-09) are applied', () => {
  const byId = new Map(list.map((x) => [x.id, x]));
  const expect = (id, pred, what) => {
    assert.ok(byId.has(id), `missing id ${id}`);
    assert.ok(pred(byId.get(id)), `${id}: ${what}`);
  };
  // §0.1.9 / §0.1.10 / §0.1.14: no own login, no own social backend, no cloudflared / WARP — obsolete ids stay but are out
  for (const id of ['account.login', 'account.logout', 'account.session-persist', 'account.register', 'account.verify-code', 'account.reset-password',
    'account.change-password', 'account.change-email', 'account.sessions-list', 'account.errors', 'library.cloud-sync', 'lists.supabase-mirror',
    'settings.cloud-sync', 'social.migrations', 'social.profile.edit', 'social.settings.privacy', 'social.notifications.realtime',
    'social.comments.episode', 'social.comments.anime', 'social.comments.compose', 'social.comments.manage', 'social.comments.manga-novel',
    'social.rooms.lobby', 'social.rooms.room', 'social.rooms.chat', 'social.rooms.queue', 'social.rooms.sync', 'social.rooms.host', 'social.rooms.invite',
    'social.rooms.each-own-source', 'social.rooms.polls', 'social.comments.rich-media', 'details.watch-together', 'manga.reader.comments-mount',
    'novel.reader.paragraph-comments-mount', 'server.cloudflared', 'server.warp', 'system.update-install']) {
    expect(id, (x) => x.status === 'out' && /§0\.1|D7|enmiendas 2026-11-09/.test(x.note ?? ''), 'must be out with a note citing plan §0.1');
  }
  // no entry may still describe Supabase as the thing to build (the removal entry itself names it, by design)
  for (const x of list) {
    if (x.status === 'out' || x.id === 'account.supabase-removal') continue;
    assert.ok(!/supabase/i.test(`${x.title} ${x.accept} ${x.next ?? ''}`), `${x.id}: a live entry still depends on Supabase (plan §0.1.9)`);
  }
  // §0.1.9 replacements: AniList / MAL identity (g/q) and resume history synced over Tatakai Link (f desktop route, q Android client)
  expect('account.identity', (x) => x.wave === 'A' && x.area === 'g' && x.android === 'q' && PLANNED.has(x.status), 'identity = AniList/MAL, wave A g/q');
  expect('account.supabase-removal', (x) => x.wave === 'A' && x.area === 'f' && x.android === 'q' && PLANNED.has(x.status), 'Supabase removal is f/q in wave A');
  expect('history.local-resume', (x) => x.wave === 'A' && x.area === 'f' && PLANNED.has(x.status), 'local resume position, wave A');
  expect('history.link-sync.route', (x) => x.wave === 'A' && x.area === 'f' && x.platform === 'desktop' && x.android === null && PLANNED.has(x.status), 'tk-link route is desktop-only, f');
  expect('history.link-sync.client', (x) => x.wave === 'A' && x.android === 'q' && x.platform === 'android' && PLANNED.has(x.status), 'Link history client is Android q');
  expect('history.link-sync.merge', (x) => x.wave === 'A' && PLANNED.has(x.status), 'merge rule in wave A');
  expect('history.link-sync.status', (x) => x.wave === 'A' && PLANNED.has(x.status), 'sync status UI in wave A');
  for (const id of ['lists.anilist.oauth', 'lists.mal.oauth', 'lists.token-refresh', 'lists.disconnect', 'account.oauth.errors']) {
    expect(id, (x) => x.wave === 'A' && x.area === 'g' && x.android === 'q' && PLANNED.has(x.status), 'AniList/MAL OAuth is g/q in wave A');
  }
  // §0.1.11: into wave A
  expect('account.setup-wizard', (x) => x.wave === 'A' && x.area === 'f' && PLANNED.has(x.status), 'first-run wizard is f, wave A');
  expect('system.reset-app', (x) => x.wave === 'A' && x.area === 'f' && PLANNED.has(x.status), 'reset app is f, wave A');
  for (const id of ['torrent.magnet-link-handler', 'torrent.session-history', 'torrent.trackers.status']) {
    expect(id, (x) => x.wave === 'A' && x.area === 'i' && x.android === 'q' && PLANNED.has(x.status), 'system magnet items are i/q, wave A');
  }
  assert.ok(list.some((x) => x.wave === 'A' && x.android === 'q' && TORRENT_ID_RE.test(x.id) && PLANNED.has(x.status)), 'q must own todo torrent/debrid ids in wave A');
  assert.ok(!list.some((x) => x.wave === 'A' && x.android === 'r'), 'no wave A entry may still name r (wave B) as Android owner');
  // §0.1.10: social is AniList-only, still n / t in wave B
  for (const id of ['social.profile.page', 'social.profile.public', 'social.follow', 'social.follow.lists', 'social.notifications.list',
    'social.notifications.badge', 'social.activity.feed', 'social.activity.interact', 'details.reviews']) {
    expect(id, (x) => x.wave === 'B' && x.area === 'n' && x.android === 't' && PLANNED.has(x.status) && /AniList/.test(`${x.title} ${x.accept}`), 'AniList-only social, n/t wave B');
  }
  expect('details.comments-mount', (x) => PLANNED.has(x.status) && /AniList/.test(x.title) && !/[Cc]omentarios/.test(x.title), 'details mount point is repurposed for AniList activity/reviews');
  // §0.1.12: background playback with Media3 and libmpv
  expect('player.android.background', (x) => x.android === 'p' && PLANNED.has(x.status) && /libmpv/.test(x.accept) && /Media3/.test(x.accept), 'background playback covers Media3 and libmpv');
  // §0.1.13: wave B additions
  expect('sources.embed-sniffer', (x) => x.wave === 'B' && x.area === 'k' && x.android === 't' && PLANNED.has(x.status), 'embed sniffer is k/t, wave B');
  for (const id of ['metadata.providers.mal', 'metadata.providers.shikimori', 'metadata.providers.tenrai', 'settings.metadata.extra-providers']) {
    expect(id, (x) => x.wave === 'B' && x.area === 'b' && PLANNED.has(x.status), 'extra metadata providers are b, wave B');
  }
  expect('novel.tts.kitten', (x) => x.wave === 'B' && x.area === 'm' && PLANNED.has(x.status) && x.platform === 'desktop', 'KittenTTS is optional, m, desktop-only');
  // accepted defaults
  expect('system.secrets-store', (x) => x.wave === 'A' && x.area === 'f' && PLANNED.has(x.status), 'secrets store is f, wave A');
  expect('system.update-check', (x) => /Kushro\/Tatakai-own/.test(x.accept), 'update feed is GitHub Releases of Kushro/Tatakai-own');
  for (const id of ['home.recs.atlas', 'player.android.upscaling-shaders']) expect(id, (x) => PLANNED.has(x.status), 'kept in the plan');
});

// Wave B contract (2026-11-09): the checklist is the only shared artifact between the 10 wave B workers, so its
// wave B rows must be unambiguous: one Android owner per desktop area (plan §2), no excluded backend named as work,
// every `todo` with an observable `accept`, and the ids the wave B contracts cite present with the right owners.
// User decision 2026-11-09 (balance r / t): Android extensions (j), the embed sniffer and the phone side of the native
// server (k `sources.*` / `server.*`) moved from r to t; k's Link ids stay with r.
const ANDROID_FOR_B_AREA = { b: 'r', h: 'r', j: 't', k: 'r', l: 's', m: 's', n: 't' };
const androidForB = (x) => (x.area === 'k' && /^(sources|server)\./.test(x.id) ? 't' : ANDROID_FOR_B_AREA[x.area]);
// Assembled from parts so the E8 guard (guard.test.mjs) does not match this file.
const EXCLUDED_BACKENDS = new RegExp(['supabase', 'cloudflared', '\\bwarp\\b', 'realtime', 'giphy', 'turnstile', 'sen' + 'try', 'data' + 'dog'].join('|'), 'i');

test('parity checklist: wave B rows are ready for the workers', () => {
  const byId = new Map(list.map((x) => [x.id, x]));
  const waveB = list.filter((x) => x.wave === 'B');
  for (const x of waveB) {
    if (!PLANNED.has(x.status)) continue;
    // plan §2 + user decision 2026-11-09: b, h, k Link -> r; j, k sources / server -> t; l, m -> s; n -> t (or desktop-only)
    if (x.android !== null) assert.equal(x.android, androidForB(x), `${x.id}: wave B area ${x.area} pairs with Android ${androidForB(x)} (plan §2), got ${x.android}`);
    // §0.1.9 / §0.1.10 / §0.1.14: nothing excluded is described as the thing to build
    assert.ok(!EXCLUDED_BACKENDS.test(`${x.title} ${x.accept}`), `${x.id}: a planned wave B entry names an excluded backend or service (plan §0.1)`);
    assert.ok(!/comentarios por episodio|ver juntos|salas?\b/i.test(x.accept), `${x.id}: own comments / watch rooms are out (plan §0.1.10)`);
    // `accept` is what the auditor (F4) and the re-review (F8) check: never an exclusion text, never "coming soon"
    assert.ok(!/^Excluido/.test(x.accept), `${x.id}: a planned entry cannot have an exclusion accept`);
    assert.ok(!/Próximamente|Coming (soon|in M\d)/i.test(x.accept) || /sin|ya no|desaparece|no (muestran|aparece)/i.test(x.accept), `${x.id}: accept must not promise a placeholder`);
  }
  // every wave B owner has todo work; Android s and t too
  for (const o of ['b', 'h', 'j', 'k', 'l', 'm', 'n']) assert.ok(waveB.some((x) => x.area === o && PLANNED.has(x.status)), `wave B owner ${o} has no planned entries`);
  for (const o of ['r', 's', 't']) assert.ok(waveB.some((x) => x.android === o && PLANNED.has(x.status)), `wave B Android owner ${o} has no planned entries`);
  const expectOwner = (id, area, android, extra = () => true) => {
    assert.ok(byId.has(id), `missing wave B id ${id}`);
    const x = byId.get(id);
    assert.ok(x.wave === 'B' && x.area === area && x.android === android && PLANNED.has(x.status) && extra(x), `${id}: expected wave B ${area}/${android} planned, got ${x.wave} ${x.area}/${x.android} ${x.status}`);
  };
  // §0.1.13 sniffer (k / r) and its two companions
  expectOwner('sources.embed-sniffer.ranking', 'k', 't');
  expectOwner('sources.embed-sniffer.adblock', 'k', 't');
  expectOwner('sources.embed-sniffer.timeout-setting', 'k', 't');
  // KNOWN-ISSUES (wave A) items deferred to wave B
  expectOwner('sources.debrid.episode-context', 'k', null, (x) => x.platform === 'desktop');
  expectOwner('link.send-to-device', 'k', 'r', (x) => /GET \/v1\/events/.test(x.accept)); // PC -> phone over the canonical /v1/events long-poll (orchestrator 2026-11-09)
  expectOwner('link.last-seen', 'k', 'r');
  // Android "M1" stubs of Dispositivo enlazado (inventory-next §3) close in wave B
  for (const id of ['link.remote-control', 'link.handoff', 'link.discovery']) expectOwner(id, 'k', 'r', (x) => /M1/.test(x.accept));
  // tk-server (doc 13) serves .kai only; interpreted ecosystems stay in server/ (D4)
  for (const id of ['server.native-mode', 'server.api.sources', 'server.api.resolve-proxy', 'server.api.invoke', 'server.api.extensions', 'server.pairing']) expectOwner(id, 'k', 't');
  assert.equal(byId.get('server.interpreted-extensions')?.status, 'out', 'server.interpreted-extensions must be out (D4)');
  // metadata providers (b / r) with per-provider status
  for (const id of ['metadata.providers.mal', 'metadata.providers.shikimori', 'metadata.providers.tenrai', 'settings.metadata.extra-providers', 'settings.metadata.provider-status']) expectOwner(id, 'b', 'r');
  // mature-content setting is b and the two adult gates read it
  expectOwner('settings.privacy.mature-content', 'b', 'r');
  assert.match(byId.get('manga.adult-gate').accept, /mature-content/, 'manga.adult-gate must read settings.privacy.mature-content');
  // downloads (h / r): page unites web + torrents and lists manga / novel chapters too
  expectOwner('downloads.page', 'h', 'r', (x) => /Manga/.test(x.accept) && /Novelas/.test(x.accept));
  expectOwner('downloads.integrity-check', 'h', 'r');
  expectOwner('downloads.android.workmanager', 'h', 'r', (x) => x.platform === 'android');
  // extensions (j / r)
  for (const id of ['ext.kill-switch', 'ext.custom-providers.continue', 'ext.stremio', 'ext.repositories', 'ext.custom-providers.home-rows']) expectOwner(id, 'j', 't');
  for (const id of ['ext.server-client.pair', 'ext.server-client.manage', 'ext.server-client.sources', 'ext.deep-link.server']) expectOwner(id, 'j', null, (x) => x.platform === 'desktop');
  // manga / novels (l, m / s); KittenTTS stays desktop-only, Android is an open question (out)
  expectOwner('manga.route', 'l', 's');
  expectOwner('novel.route', 'm', 's');
  expectOwner('novel.tts.system', 'm', 's');
  assert.equal(byId.get('novel.tts.kitten.android')?.status, 'out', 'KittenTTS on Android is out (plan §0.1.13: desktop only)');
  // home (b / r): the audit "Parcial" rows of Inicio are planned
  for (const id of ['home.latest-episodes', 'home.top10', 'home.upcoming', 'home.most-favorite', 'home.because-you-watched', 'home.genre-cloud', 'home.infinite-genre-rows',
    'home.calendar.schedule', 'home.recs.for-you', 'settings.appearance.more-palettes']) expectOwner(id, 'b', 'r');
});
