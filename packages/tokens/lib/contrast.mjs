// WCAG 2.x relative luminance / contrast ratio on sRGB hex colors.
const lin = (c) => {
  c /= 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};

export function luminance(hex) {
  const n = parseInt(hex.slice(1), 16);
  return 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
}

export function contrastRatio(a, b) {
  const la = luminance(a);
  const lb = luminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

/** E12: the exact pairs that must reach 4.5:1 in light / dark / amoled. [foreground, background] */
export const AA_PAIRS = [
  ['onPrimary', 'primary'],
  ['onSecondary', 'secondary'],
  ['onTertiary', 'tertiary'],
  ['onError', 'error'],
  ['onSurface', 'surface'],
  ['onSurfaceVariant', 'surface'],
  ['onPrimaryContainer', 'primaryContainer'],
  ['onSecondaryContainer', 'secondaryContainer'],
  ['onTertiaryContainer', 'tertiaryContainer'],
];

export const AA_MIN = 4.5;
