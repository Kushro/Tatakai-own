import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

// Guard (E8): the native apps, shared packages and contracts must never reference the legacy backend, crash reporting or telemetry.
// Scanned roots: apps/ packages/ contracts/ (includes apps/**/packaging/*.ps1). Case-insensitive.
// The pattern is assembled from parts so this file does not match itself; it is still allow-listed explicitly.
const PATTERN = new RegExp(['tatakai\.me', 'tatakaiapi', 'sen' + 'try', 'data' + 'dog', 'analy' + 'tics'].join('|'), 'i');

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const ROOTS = ['apps', 'packages', 'contracts'];
const SKIP_DIRS = new Set(['node_modules', 'build', '.gradle', '.deps-sources', 'target', '.git', '.idea', 'captures']);
// Docs that explain the rule itself may name the forbidden words.
const ALLOWLIST = new Set(['apps/README.md', 'contracts/test/guard.test.mjs'].map((p) => p.split('/').join(sep)));
const MAX_BYTES = 3 * 1024 * 1024;

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (!SKIP_DIRS.has(name)) yield* walk(full);
    } else if (st.size <= MAX_BYTES) {
      yield full;
    }
  }
}

test('no tatakai.me / TatakaiAPI / crash-reporting / telemetry references under apps/ packages/ contracts/', () => {
  const hits = [];
  let scanned = 0;
  for (const root of ROOTS) {
    const abs = join(repo, root);
    for (const file of walk(abs)) {
      const rel = relative(repo, file);
      if (ALLOWLIST.has(rel)) continue;
      const buf = readFileSync(file);
      if (buf.includes(0)) continue; // binary (jar, zip, wrapper)
      scanned++;
      const lines = buf.toString('utf8').split('\n');
      lines.forEach((line, i) => {
        if (PATTERN.test(line)) hits.push(`${rel}:${i + 1}: ${line.trim().slice(0, 120)}`);
      });
    }
  }
  assert.ok(scanned > 50, `guard scanned suspiciously few files (${scanned})`);
  assert.deepEqual(hits, []);
});

test('guard pattern really matches the forbidden words (self-test)', () => {
  for (const sample of ['https://api.tatakai.me/x', 'TatakaiAPI', 'Sen' + 'try.init', 'DATA' + 'DOG', 'google-analy' + 'tics']) assert.match(sample, PATTERN);
  assert.doesNotMatch('tatakai-link:v1', PATTERN);
});
