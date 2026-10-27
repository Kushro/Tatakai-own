import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { validate } from './helpers/validate.mjs';
import { here, loadTokens, generateAll, OUTPUTS } from '../generate.mjs';
import { computeAll, ROLES, MODES, CONTRAST_LEVELS } from '../lib/colors.mjs';
import { AA_PAIRS, AA_MIN, contrastRatio } from '../lib/contrast.mjs';
import { springToCss } from '../lib/spring.mjs';

const schema = JSON.parse(readFileSync(join(here, 'tokens.schema.json'), 'utf8'));
const tokens = loadTokens();
const normalize = (s) => s.replace(/\r\n/g, '\n');

const EXPECTED_PALETTES = ['tatakai-violet', 'midnight', 'sakura', 'matcha', 'ocean', 'neon-tokyo', 'blood-moon', 'royal-gold', 'aurora', 'mono'];

test('tokens.json is valid against tokens.schema.json', () => {
  assert.deepEqual(validate(schema, tokens), []);
});

test('schema rejects broken tokens (validator sanity)', () => {
  const bad = structuredClone(tokens);
  bad.palettes.sakura.seed = 'E86A92';
  bad.palettes.sakura.style = 'Rainbow';
  delete bad.fonts;
  const errors = validate(schema, bad);
  assert.ok(errors.some((e) => e.includes('seed')), errors.join('\n'));
  assert.ok(errors.some((e) => e.includes('style')), errors.join('\n'));
  assert.ok(errors.some((e) => e.includes('fonts')), errors.join('\n'));
});

test('has the 10 curated palettes of 01-design-system and a valid default', () => {
  assert.deepEqual(Object.keys(tokens.palettes), EXPECTED_PALETTES);
  assert.ok(tokens.palettes[tokens.defaultPalette]);
  assert.equal(tokens.palettes.mono.style, 'Monochrome');
  assert.equal(tokens.palettes.mono.contrast, 1);
});

test('allowContrastFailures only references existing palettes and roles', () => {
  for (const e of tokens.allowContrastFailures) {
    assert.ok(e.palette === '*' || tokens.palettes[e.palette], `unknown palette ${e.palette}`);
    for (const role of e.pair) assert.ok(ROLES.includes(role), `unknown role ${role}`);
  }
});

test('every scheme defines all 48 ColorScheme roles as #RRGGBB', () => {
  assert.equal(ROLES.length, 48);
  const all = computeAll(tokens);
  for (const [id, levels] of Object.entries(all)) {
    for (const level of CONTRAST_LEVELS) {
      for (const mode of MODES) {
        const scheme = levels[level][mode];
        for (const role of ROLES) assert.match(scheme[role], /^#[0-9A-F]{6}$/, `${id}/${level}/${mode}/${role}`);
      }
    }
  }
});

test('amoled: black surfaces, same accent roles as dark', () => {
  const all = computeAll(tokens);
  for (const [id, levels] of Object.entries(all)) {
    const { dark, amoled } = levels.standard;
    assert.equal(amoled.surface, '#000000', id);
    assert.equal(amoled.background, '#000000', id);
    assert.equal(amoled.surfaceContainerLowest, '#000000', id);
    assert.equal(amoled.primary, dark.primary, id);
    assert.equal(amoled.onSurface, dark.onSurface, id);
  }
});

test('regenerating Tokens.kt and tokens.css produces no diff (committed files are current)', () => {
  const fresh = generateAll(tokens);
  for (const key of ['kotlin', 'css']) {
    assert.ok(existsSync(OUTPUTS[key]), `missing ${OUTPUTS[key]} - run: node packages/tokens/generate.mjs`);
    const committed = normalize(readFileSync(OUTPUTS[key], 'utf8'));
    assert.equal(committed, fresh[key], `${OUTPUTS[key]} is out of date - run: node packages/tokens/generate.mjs`);
  }
});

test('generation is deterministic', () => {
  assert.deepEqual(generateAll(tokens), generateAll(tokens));
});

test('Tokens.kt exposes the contract of 04-implementation-plan (names and shapes)', () => {
  const kt = generateAll(tokens).kotlin;
  for (const needle of [
    'object TatakaiTokens', 'data class PaletteSchemes(val light: ColorScheme, val dark: ColorScheme, val amoled: ColorScheme)',
    'val palettes: Map<String, PaletteSchemes>', 'val shapes: Shapes', 'val motion: MotionScheme', 'val spacing: Spacing',
    'fun typography(fonts: TatakaiFontFamilies', 'surfaceContainerLowest = ', 'tertiaryFixedDim = ', 'onTertiaryFixedVariant = ',
  ]) assert.ok(kt.includes(needle), `missing: ${needle}`);
  const schemes = kt.match(/^private fun scheme\w+\(\): ColorScheme =/gm) ?? [];
  assert.equal(schemes.length, tokens.palettes ? Object.keys(tokens.palettes).length * MODES.length * CONTRAST_LEVELS.length : 0);
});

test('tokens.css exposes the documented custom properties and selectors', () => {
  const css = generateAll(tokens).css;
  for (const needle of [
    '--md-sys-color-on-primary-container:', '--md-sys-color-surface-container-lowest:', '--md-sys-typescale-display-large-size: 57px',
    '--md-sys-shape-corner-large-increased: 20px', '--md-sys-motion-spatial-default-easing: linear(', '--md-sys-motion-effects-fast-duration:',
    '--tk-space-4: 4px', ':root[data-palette="sakura"][data-mode="amoled"] {', ':root[data-palette="mono"][data-mode="light"][data-contrast="high"] {',
  ]) assert.ok(css.includes(needle), `missing: ${needle}`);
});

test('spring -> CSS linear() easing starts at 0, ends at 1 and is bounded', () => {
  for (const kind of ['spatial', 'effects']) {
    for (const speed of ['fast', 'default', 'slow']) {
      const { durationMs, easing } = springToCss(tokens.motion[kind][speed]);
      const points = easing.slice('linear('.length, -1).split(', ').map(Number);
      assert.equal(points[0], 0);
      assert.equal(points.at(-1), 1);
      assert.ok(durationMs >= 50 && durationMs <= 2000, `${kind}.${speed} duration ${durationMs}`);
      assert.ok(points.every((p) => p > -0.01 && p < 1.3));
    }
  }
});

// ---- E12: WCAG AA (4.5:1) on the exact role pairs, light / dark / amoled, with explicit exceptions only ----
const isAllowed = (id, level, mode, pair) =>
  tokens.allowContrastFailures.some(
    (e) =>
      (e.palette === '*' || e.palette === id) &&
      (e.mode === '*' || e.mode === mode) &&
      (!e.contrast || e.contrast === '*' || e.contrast === level) &&
      e.pair[0] === pair[0] && e.pair[1] === pair[1],
  );

test('contrast: all E12 pairs reach 4.5:1 for every palette x mode x contrast level (except declared exceptions)', () => {
  const all = computeAll(tokens);
  const failures = [];
  for (const [id, levels] of Object.entries(all)) {
    for (const level of CONTRAST_LEVELS) {
      for (const mode of MODES) {
        const scheme = levels[level][mode];
        for (const pair of AA_PAIRS) {
          const ratio = contrastRatio(scheme[pair[0]], scheme[pair[1]]);
          if (ratio < AA_MIN && !isAllowed(id, level, mode, pair)) {
            failures.push(`${id}/${level}/${mode}: ${pair[0]} on ${pair[1]} = ${ratio.toFixed(2)} (${scheme[pair[0]]} on ${scheme[pair[1]]})`);
          }
        }
      }
    }
  }
  assert.deepEqual(failures, []);
});

test('contrast: pair list is exactly the 9 pairs of review E12', () => {
  assert.deepEqual(AA_PAIRS.map((p) => p.join('|')), [
    'onPrimary|primary', 'onSecondary|secondary', 'onTertiary|tertiary', 'onError|error',
    'onSurface|surface', 'onSurfaceVariant|surface',
    'onPrimaryContainer|primaryContainer', 'onSecondaryContainer|secondaryContainer', 'onTertiaryContainer|tertiaryContainer',
  ]);
});

test('contrast ratio helper matches known WCAG values', () => {
  assert.equal(contrastRatio('#000000', '#FFFFFF').toFixed(2), '21.00');
  assert.equal(contrastRatio('#777777', '#FFFFFF').toFixed(2), '4.48');
});
