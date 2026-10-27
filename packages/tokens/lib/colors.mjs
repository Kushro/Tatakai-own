// Expands palette seeds into full Material 3 (1.5) color roles for light / dark / amoled at three contrast levels.
// Uses the vendored @material/material-color-utilities (Apache-2.0), see ../vendor/material-color-utilities/README.md
import * as mcu from '../vendor/material-color-utilities/index.js';

/** Exact order of the androidx.compose.material3.ColorScheme 1.5 constructor (without the deprecated overloads). */
export const ROLES = [
  'primary', 'onPrimary', 'primaryContainer', 'onPrimaryContainer', 'inversePrimary',
  'secondary', 'onSecondary', 'secondaryContainer', 'onSecondaryContainer',
  'tertiary', 'onTertiary', 'tertiaryContainer', 'onTertiaryContainer',
  'background', 'onBackground', 'surface', 'onSurface', 'surfaceVariant', 'onSurfaceVariant',
  'surfaceTint', 'inverseSurface', 'inverseOnSurface',
  'error', 'onError', 'errorContainer', 'onErrorContainer',
  'outline', 'outlineVariant', 'scrim',
  'surfaceBright', 'surfaceDim', 'surfaceContainer', 'surfaceContainerHigh', 'surfaceContainerHighest',
  'surfaceContainerLow', 'surfaceContainerLowest',
  'primaryFixed', 'primaryFixedDim', 'onPrimaryFixed', 'onPrimaryFixedVariant',
  'secondaryFixed', 'secondaryFixedDim', 'onSecondaryFixed', 'onSecondaryFixedVariant',
  'tertiaryFixed', 'tertiaryFixedDim', 'onTertiaryFixed', 'onTertiaryFixedVariant',
];

export const MODES = ['light', 'dark', 'amoled'];
export const CONTRAST_LEVELS = ['standard', 'medium', 'high'];

const STYLES = {
  Expressive: mcu.SchemeExpressive,
  TonalSpot: mcu.SchemeTonalSpot,
  Vibrant: mcu.SchemeVibrant,
  Fidelity: mcu.SchemeFidelity,
  Monochrome: mcu.SchemeMonochrome,
};

/** contrast value (-1..1) used for a palette at a user-facing level. standard = the palette's own base contrast. */
export function contrastValue(palette, level) {
  if (level === 'standard') return palette.contrast;
  if (level === 'medium') return Math.max(0.5, palette.contrast);
  if (level === 'high') return 1.0;
  throw new Error(`unknown contrast level ${level}`);
}

const hex = (argb) => mcu.hexFromArgb(argb).toUpperCase();
const parseHex = (h) => mcu.argbFromHex(h);

/**
 * AMOLED post-process (documented in docs/redesign/01-design-system.md, section 2): start from the DARK scheme and
 *  - background, surface, surfaceDim, surfaceContainerLowest -> #000000
 *  - the remaining surface containers are re-picked from the neutral tonal palette at lower tones:
 *    Low T5, Container T8, High T12, Highest T16, Bright T20 (dark uses T10/T12/T17/T22/T24).
 * Every other role (including the on* roles) is left untouched, so on* vs surface contrast can only go up.
 */
function amoledOverrides(scheme) {
  const n = (tone) => scheme.neutralPalette.tone(tone);
  const black = 0xff000000;
  return {
    background: black, surface: black, surfaceDim: black, surfaceContainerLowest: black,
    surfaceContainerLow: n(5), surfaceContainer: n(8), surfaceContainerHigh: n(12),
    surfaceContainerHighest: n(16), surfaceBright: n(20),
  };
}

/** Returns { role: '#RRGGBB' } with every role in ROLES. */
export function computeScheme(palette, mode, level = 'standard') {
  if (!MODES.includes(mode)) throw new Error(`unknown mode ${mode}`);
  const Style = STYLES[palette.style];
  if (!Style) throw new Error(`unknown palette style ${palette.style}`);
  const isDark = mode !== 'light';
  const scheme = new Style(mcu.Hct.fromInt(parseHex(palette.seed)), isDark, contrastValue(palette, level));
  const dc = new mcu.MaterialDynamicColors();
  const out = {};
  for (const role of ROLES) {
    if (typeof dc[role] !== 'function') throw new Error(`MaterialDynamicColors has no role ${role}`);
    out[role] = dc[role]().getArgb(scheme);
  }
  if (mode === 'amoled') Object.assign(out, amoledOverrides(scheme));
  const result = {};
  for (const role of ROLES) result[role] = hex(out[role]);
  const ov = palette.overrides ?? {};
  // overrides.dark also applies to amoled unless amoled overrides it again
  const applied = mode === 'amoled' ? { ...(ov.dark ?? {}), ...(ov.amoled ?? {}) } : (ov[mode] ?? {});
  for (const [role, value] of Object.entries(applied)) result[role] = value.toUpperCase();
  return result;
}

/** { paletteId: { standard: { light, dark, amoled }, medium: {...}, high: {...} } } in tokens.json order. */
export function computeAll(tokens) {
  const all = {};
  for (const [id, palette] of Object.entries(tokens.palettes)) {
    all[id] = {};
    for (const level of CONTRAST_LEVELS) {
      all[id][level] = {};
      for (const mode of MODES) all[id][level][mode] = computeScheme(palette, mode, level);
    }
  }
  return all;
}
