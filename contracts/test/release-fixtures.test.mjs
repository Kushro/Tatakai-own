import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Shape of the shared release fixtures (docs/redesign/12 §3.1): the Kotlin :core:release tests (and later `cargo test -p tk-core`) read
// them as the oracle of parseRelease / pickEpisodeFile. This test only checks the format, not the parser.
const dir = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'fixtures', 'release');
const readJson = (name) => JSON.parse(readFileSync(join(dir, name), 'utf8'));

const RESOLUTIONS = new Set(['unknown', '480p', '720p', '1080p', '1440p', '2160p']);
const SOURCES = new Set(['bd', 'remux', 'web', 'tv', 'dvd']);
const CODECS = new Set(['h264', 'hevc', 'av1', 'vp9']);
const nullableInt = (v) => v === null || (Number.isInteger(v) && v >= 0);
const CHECKS = {
  group: (v) => v === null || typeof v === 'string',
  title: (v) => v === null || typeof v === 'string',
  season: nullableInt,
  episodes: (v) => v === null || (Array.isArray(v) && v.length === 2 && v.every(Number.isInteger) && v[0] <= v[1]),
  source: (v) => v === null || SOURCES.has(v),
  resolution: (v) => RESOLUTIONS.has(v),
  codec: (v) => v === null || CODECS.has(v),
  bitDepth: nullableInt,
  dualAudio: (v) => typeof v === 'boolean',
  multiSub: (v) => typeof v === 'boolean',
  batch: (v) => typeof v === 'boolean',
  seadexBest: (v) => v === false,
};

test('release/names.json: >= 40 cases with partial ReleaseTags in desktop serde form', () => {
  const doc = readJson('names.json');
  assert.equal(typeof doc.$comment, 'string');
  assert.ok(doc.cases.length >= 40, `${doc.cases.length} cases`);
  for (const c of doc.cases) {
    assert.equal(typeof c.name, 'string');
    assert.equal(typeof c.from, 'string');
    for (const [k, v] of Object.entries(c.expected)) {
      assert.ok(k in CHECKS, `${c.name}: unknown key ${k}`);
      assert.ok(CHECKS[k](v), `${c.name}: bad ${k} = ${JSON.stringify(v)}`);
    }
  }
});

test('release/packs.json: >= 10 cases whose expected index exists in the file list', () => {
  const doc = readJson('packs.json');
  assert.equal(typeof doc.$comment, 'string');
  assert.ok(doc.cases.length >= 10, `${doc.cases.length} cases`);
  for (const c of doc.cases) {
    assert.equal(typeof c.label, 'string');
    assert.ok(Number.isInteger(c.episode) && c.episode >= 1, c.label);
    assert.ok(Array.isArray(c.titles) && c.titles.every((t) => typeof t === 'string'), c.label);
    for (const f of c.files) {
      assert.ok(Number.isInteger(f.index) && typeof f.path === 'string' && Number.isInteger(f.sizeBytes), c.label);
    }
    if (c.expected !== null) assert.ok(c.files.some((f) => f.index === c.expected), `${c.label}: index ${c.expected} not in files`);
  }
});
