#!/usr/bin/env node
// E2: downloads the *-sources.jar of the alpha/unstable Compose-ish APIs and extracts them next to the jars so that
// agents can `grep` real signatures instead of guessing. Output dir is gitignored: apps/android-native/.deps-sources/
// Usage: node apps/android-native/scripts/fetch-deps-sources.mjs
import { mkdir, writeFile, access } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '.deps-sources');
const G = 'https://dl.google.com/dl/android/maven2';
// [group path, artifact, version]  (keep in sync with gradle/libs.versions.toml)
const artifacts = [
  ['androidx/compose/material3', 'material3-android', '1.5.0-alpha15'],
  ['androidx/compose/material3', 'material3-adaptive-navigation-suite-android', '1.5.0-alpha15'],
  ['androidx/navigation3', 'navigation3-runtime-android', '1.0.1'],
  ['androidx/navigation3', 'navigation3-ui-android', '1.0.1'],
  ['androidx/compose/material3/adaptive', 'adaptive-navigation3-android', '1.3.0'],
  ['androidx/media3', 'media3-ui-compose', '1.11.1'],
];

const exists = (p) => access(p).then(() => true, () => false);
for (const [group, name, version] of artifacts) {
  const file = `${name}-${version}-sources.jar`;
  const jar = join(root, file);
  const dir = join(root, `${name}-${version}`);
  if (await exists(dir)) { console.log(`ok   ${name}-${version}`); continue; }
  await mkdir(dir, { recursive: true });
  const res = await fetch(`${G}/${group}/${name}/${version}/${file}`);
  if (!res.ok) { console.error(`FAIL ${file}: HTTP ${res.status}`); process.exitCode = 1; continue; }
  await writeFile(jar, Buffer.from(await res.arrayBuffer()));
  execFileSync('jar', ['xf', jar], { cwd: dir }); // JDK's `jar` extracts zip/jar on every OS (JDK is a prerequisite anyway)
  console.log(`got  ${name}-${version}`);
}
