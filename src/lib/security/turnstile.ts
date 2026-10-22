/**
 * Cloudflare Turnstile — client helpers.
 *
 * Scope (Phase 4, "Auth + high-value forms"): the auth flow (Supabase verifies
 * the token natively via options.captchaToken) plus the suggestion and report
 * forms, which write straight to Supabase from the browser. For those forms the
 * client fetches a token, only requires that a token was produced (the upstream
 * /security/turnstile/verify endpoint is not used in this fork). The DB automod +
 * rate-limit backstop (20260925000600_ugc_guard.sql) remains the real guard.
 *
 * Degrades gracefully:
 *  - No VITE_TURNSTILE_SITE_KEY   → disabled; callers proceed without a token.
 *  - Desktop file:// (null origin) → disabled; the widget can't render there and
 *    the siteverify would reject a null hostname, so we skip it entirely.
 *  - Native iOS → disabled for the packaged app; web Safari remains enabled.
 */

import { Capacitor } from '@capacitor/core';

export const TURNSTILE_SITE_KEY = String(
  import.meta.env.VITE_TURNSTILE_SITE_KEY || ""
).trim();

/** The Turnstile script URL (explicit-render mode). */
export const TURNSTILE_SCRIPT_SRC =
  "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/**
 * Whether Turnstile should be presented/enforced in this environment. False when
 * no site key is configured or we're running from a file:// origin (desktop
 * build), where the widget can't render and siteverify rejects the null host.
 */
export function isTurnstileEnabled(): boolean {
  if (Capacitor.isNativePlatform() && Capacitor.getPlatform() === 'ios') return false;
  if (!TURNSTILE_SITE_KEY) return false;
  if (typeof window !== "undefined" && window.location.protocol === "file:") return false;
  return true;
}

/**
 * Backend verification (`/security/turnstile/verify`) belonged to the upstream
 * TatakaiAPI and is disabled in this fork. Supabase verifies captcha tokens for
 * auth natively; for the other forms the DB automod/rate-limit backstop applies.
 *  - Disabled env → true (nothing to verify).
 *  - Enabled but no token → false (widget not solved).
 *  - Enabled with a token → true (no server-side verification available).
 */
export async function verifyTurnstileToken(token: string | null | undefined): Promise<boolean> {
  if (!isTurnstileEnabled()) return true;
  return !!token;
}
