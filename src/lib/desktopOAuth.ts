/**
 * Native OAuth helpers (Electron desktop + Capacitor Android).
 *
 * Account linking runs entirely on the device:
 *   1. The app builds the provider's authorize URL and opens it in the SYSTEM
 *      browser (`openOAuthInBrowser`).
 *   2. The provider redirects to a `tatakai://integration/<provider>/redirect`
 *      deep link, registered as the OAuth redirect URL in the provider dashboard
 *      (AniList: implicit grant, token in the URL fragment; MAL: PKCE code in the
 *      query).
 *   3. The OS hands the deep link to the app (desktop: desktop/window/deep-link.cjs
 *      -> DesktopDeepLinkBridge; Android: MobileDeepLinkBridge), which routes it to
 *      `/integration/<provider>/redirect` with the query AND fragment intact.
 *
 * There is no https bridge page any more. A plain web build cannot receive the
 * `tatakai://` redirect; the settings UI says so instead of failing.
 */

import { Capacitor } from '@capacitor/core';
import { Browser } from '@capacitor/browser';
import { ANILIST_DEFAULT_REDIRECT_URI, getAniListRedirectUri } from '@/lib/anilist/auth';
import { MAL_DEFAULT_REDIRECT_URI, getMalRedirectUri } from '@/lib/mal/auth';

type ElectronBridge = { openExternal?: (url: string) => Promise<unknown> };

/** Packaged apps hand OAuth to a system browser, preserving the app WebView. */
export function isNativeOAuthApp(): boolean {
  return Capacitor.isNativePlatform() || isDesktopApp();
}

export function getElectronBridge(): ElectronBridge | undefined {
  return (window as any)?.electron;
}

/** True when running inside the Electron desktop shell. */
export function isDesktopApp(): boolean {
  if (typeof window === 'undefined') return false;
  return window.location.protocol === 'file:' || !!getElectronBridge()?.openExternal;
}

/** Redirect URLs to register in the provider dashboards (env overrides, else tatakai://). */
export const DESKTOP_ANILIST_REDIRECT_URI = ANILIST_DEFAULT_REDIRECT_URI;
export const DESKTOP_MAL_REDIRECT_URI = MAL_DEFAULT_REDIRECT_URI;

/** AniList redirect URL (implicit grant). */
export function getWebAnilistRedirectUri(): string {
  return getAniListRedirectUri();
}

/** MAL redirect URL (PKCE). */
export function getWebMalRedirectUri(): string {
  return getMalRedirectUri();
}

/** MangaBaka is disabled in this fork; kept so old imports compile. */
export function getWebMangabakaRedirectUri(): string {
  return 'tatakai://integration/mangabaka/redirect';
}

export type IntegrationAvailability = 'ready' | 'not_configured' | 'native_only';

/**
 * Whether a provider's Connect button can work in this build:
 *  - not_configured: no client id in the build env
 *  - native_only: needs the `tatakai://` deep link, i.e. the desktop or Android app
 */
export function integrationAvailability(configured: boolean): IntegrationAvailability {
  if (!configured) return 'not_configured';
  return isNativeOAuthApp() ? 'ready' : 'native_only';
}

/** Prefix marking an OAuth flow as desktop-initiated. Kept for old callers. */
export const DESKTOP_OAUTH_STATE_PREFIX = 'tatakai-desktop-';

/** Builds a desktop `state` marker. Safe chars only. */
export function buildDesktopOAuthState(): string {
  const rand = Array.from(crypto.getRandomValues(new Uint8Array(24)), byte => byte.toString(16).padStart(2, '0')).join('');
  return `${DESKTOP_OAUTH_STATE_PREFIX}${rand}`;
}

/** True when `state` carries the desktop marker. */
export function isDesktopOAuthState(state: string | null | undefined): boolean {
  return typeof state === 'string' && state.startsWith(DESKTOP_OAUTH_STATE_PREFIX);
}

/** Turns an in-app route (`/integration/...?code=…`) into a `tatakai://` URL. */
export function buildDesktopDeepLink(route: string): string {
  const path = String(route || '').replace(/^\/*/, '');
  return `tatakai://${path}`;
}

/**
 * Opens an OAuth authorize URL in the user's default system browser.
 * Returns true if handed off to the native shell, false otherwise.
 */
export async function openOAuthInBrowser(url: string): Promise<boolean> {
  if (Capacitor.isNativePlatform()) {
    await Browser.open({ url });
    return true;
  }
  const bridge = getElectronBridge();
  if (bridge?.openExternal) {
    await bridge.openExternal(url);
    return true;
  }
  return false;
}
