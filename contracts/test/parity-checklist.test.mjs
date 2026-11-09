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
const OPTIONAL = ['audit', 'platform', 'note'];
const ID_RE = /^[a-z0-9]+(-[a-z0-9]+)*(\.[a-z0-9]+(-[a-z0-9]+)*)+$/;

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
  for (const x of list) if (x.status === 'todo') todoByArea[x.area] = (todoByArea[x.area] ?? 0) + 1;
  for (const w of WAVES) for (const o of DESKTOP_OWNERS[w]) assert.ok((todoByArea[o] ?? 0) > 0, `owner ${o} has no todo entries`);
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
    assert.equal(byId.get(id).status, 'todo', `${id} must start as todo`);
  }
  // plan §7 exclusions are explicit
  for (const id of ['manga.yomi', 'social.wrapped', 'social.community.feed', 'system.admin', 'system.tv-mode', 'library.playlists', 'library.tierlists',
    'ext.store', 'system.dynamic-island', 'system.popups', 'ext.renderer']) {
    assert.ok(byId.has(id), `missing §7 exclusion ${id}`);
    assert.equal(byId.get(id).status, 'out', `${id} must be out`);
  }
});
