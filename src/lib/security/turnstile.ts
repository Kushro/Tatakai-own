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
  // Packaged iOS cannot render the widget on capacitor://; it needs an HTTPS
  // page we host ourselves (VITE_TURNSTILE_HOSTED_URL), otherwise stay disabled.
  if (Capacitor.isNativePlatform() && Capacitor.getPlatform() === 'ios' && !HOSTED_TURNSTILE_URL) return false;
  if (!TURNSTILE_SITE_KEY) return false;
  if (typeof window !== "undefined" && window.location.protocol === "file:") return false;
  return true;
}

/** WKWebView's custom-scheme origin cannot host the challenge directly. */
export function needsHostedTurnstile(): boolean {
  return Boolean(HOSTED_TURNSTILE_URL) && Capacitor.isNativePlatform() && Capacitor.getPlatform() === 'ios';
}

/** Operator-hosted widget page; empty in this fork (the upstream site is not used). */
export const HOSTED_TURNSTILE_URL = String(import.meta.env.VITE_TURNSTILE_HOSTED_URL || '').trim();

export type HostedTurnstileMessage = {
  type: 'tatakai-turnstile';
  id: string;
  event: 'ready' | 'token' | 'expired' | 'error' | 'timeout';
  token?: string;
};

/** Accept tokens only from this widget's HTTPS frame and current render. */
export function readHostedTurnstileMessage(
  event: MessageEvent,
  source: Window | null,
  id: string,
): HostedTurnstileMessage | null {
  if (!HOSTED_TURNSTILE_URL || !source || event.source !== source || event.origin !== new URL(HOSTED_TURNSTILE_URL).origin) return null;
  const data = event.data;
  if (!data || data.type !== 'tatakai-turnstile' || data.id !== id) return null;
  if (!['ready', 'token', 'expired', 'error', 'timeout'].includes(data.event)) return null;
  if (data.event === 'token' && (typeof data.token !== 'string' || !data.token || data.token.length > 2048)) return null;
  return data;
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
