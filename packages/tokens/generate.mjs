#!/usr/bin/env node
// tokens.json -> Tokens.kt + tokens.css. Plain Node, no npm dependencies (material-color-utilities is vendored).
//   node packages/tokens/generate.mjs           write both files
//   node packages/tokens/generate.mjs --check   exit 1 if the committed files differ from a fresh generation
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { computeAll } from './lib/colors.mjs';
import { emitKotlin } from './lib/emit-kotlin.mjs';
import { emitCss } from './lib/emit-css.mjs';

export const here = dirname(fileURLToPath(import.meta.url));
export const repoRoot = resolve(here, '..', '..');
export const TOKENS_PATH = join(here, 'tokens.json');
export const OUTPUTS = {
  kotlin: join(repoRoot, 'apps/android-native/core/designsystem/src/main/kotlin/app/tatakai/next/designsystem/generated/Tokens.kt'),
  css: join(repoRoot, 'apps/desktop-native/crates/tk-ui/assets/tokens.css'),
};

export function loadTokens() {
  return JSON.parse(readFileSync(TOKENS_PATH, 'utf8'));
}

/** @returns {{kotlin: string, css: string}} contents with LF newlines */
export function generateAll(tokens = loadTokens()) {
  const all = computeAll(tokens);
  return { kotlin: emitKotlin(tokens, all), css: emitCss(tokens, all) };
}

const normalize = (s) => s.replace(/\r\n/g, '\n');

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes('--check');
  const generated = generateAll();
  let dirty = false;
  for (const key of Object.keys(OUTPUTS)) {
    const path = OUTPUTS[key];
    if (check) {
      const current = existsSync(path) ? normalize(readFileSync(path, 'utf8')) : null;
      if (current !== generated[key]) {
        console.error(`out of date: ${path}`);
        dirty = true;
      }
    } else {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, generated[key], 'utf8');
      console.log(`wrote ${path}`);
    }
  }
  if (dirty) process.exit(1);
}
