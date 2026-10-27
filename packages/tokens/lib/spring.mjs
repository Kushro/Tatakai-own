// Compose spring (mass = 1) -> CSS `linear()` easing + duration, so desktop motion matches Android's MotionScheme.
// x(t) is the unit step response of a damped oscillator with natural frequency w0 = sqrt(stiffness), ratio zeta.
export function springPosition(t, { damping: zeta, stiffness }) {
  const w0 = Math.sqrt(stiffness);
  if (zeta < 1) {
    const wd = w0 * Math.sqrt(1 - zeta * zeta);
    return 1 - Math.exp(-zeta * w0 * t) * (Math.cos(wd * t) + ((zeta * w0) / wd) * Math.sin(wd * t));
  }
  if (zeta === 1) return 1 - Math.exp(-w0 * t) * (1 + w0 * t);
  const s = Math.sqrt(zeta * zeta - 1);
  const r1 = -w0 * (zeta - s);
  const r2 = -w0 * (zeta + s);
  return 1 - (r2 * Math.exp(r1 * t) - r1 * Math.exp(r2 * t)) / (r2 - r1);
}

/** @returns {{ durationMs: number, easing: string }} */
export function springToCss(spring, points = 32, epsilon = 0.002) {
  let settled = 0;
  for (let ms = 1; ms <= 4000; ms++) {
    if (Math.abs(springPosition(ms / 1000, spring) - 1) > epsilon) settled = ms;
  }
  const durationMs = Math.max(50, Math.ceil((settled + 1) / 10) * 10);
  const samples = [];
  for (let i = 0; i < points; i++) {
    const x = springPosition((durationMs / 1000) * (i / (points - 1)), spring);
    samples.push(i === points - 1 ? '1' : String(Math.round(x * 10000) / 10000));
  }
  samples[0] = '0';
  return { durationMs, easing: `linear(${samples.join(', ')})` };
}
