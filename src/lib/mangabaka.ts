/**
 * MangaBaka tracker integration — DISABLED in this fork.
 *
 * MangaBaka's OAuth token exchange and library API required the TatakaiAPI
 * server (client secret + `mangabaka_credentials` table), which this fork does
 * not use. Every function here resolves to a "not available in this build"
 * state without touching the network, and the settings UI hides the Connect
 * button. Exports and types are kept so existing imports keep compiling.
 */

export const MANGABAKA_LOGO_URL = 'https://mangabaka.org/images/logo.png';
/** MangaBaka brand accent. */
export const MANGABAKA_ACCENT = '#e30047';
export const MANGABAKA_SITE_URL = 'https://mangabaka.org';
export const MANGABAKA_SETTINGS_URL = 'https://mangabaka.org/my/settings';
export const MANGABAKA_LIBRARY_URL = 'https://mangabaka.org/my/library';

/** True when MangaBaka can be linked in this build. Always false in this fork. */
export const MANGABAKA_AVAILABLE = false;
export const MANGABAKA_UNAVAILABLE_MESSAGE = 'MangaBaka is not available in this build.';

/** Canonical redirect URI placeholder (kept for compatibility; unused). */
export const MANGABAKA_CANONICAL_REDIRECT_URI = 'tatakai://integration/mangabaka/redirect';

export class MangaBakaUnavailableError extends Error {
  constructor() {
    super(MANGABAKA_UNAVAILABLE_MESSAGE);
    this.name = 'MangaBakaUnavailableError';
  }
}

export async function getMangaBakaAuthUrl(_redirectUriOverride?: string, _stateOverride?: string): Promise<string> {
  throw new MangaBakaUnavailableError();
}

export function isMangaBakaLoginPendingFresh(): boolean {
  return false;
}

export function clearMangaBakaOAuthStorage(): void {
  /* nothing is stored */
}

export function clearMangaBakaPendingMarker(): void {
  /* nothing is stored */
}

export function isMangaBakaConnectedCached(): boolean {
  return false;
}

export function readMangaBakaCachedStatus(): { connected: boolean; username: string | null } {
  return { connected: false, username: null };
}

export function setMangaBakaConnectedCached(_connected: boolean, _username?: string | null): void {
  /* nothing is stored */
}

export function assertWebMangabakaRedirectMatchesOrigin(_redirectUri: string): void {
  throw new MangaBakaUnavailableError();
}

export async function exchangeMangaBakaCode(_code: string): Promise<never> {
  throw new MangaBakaUnavailableError();
}

export interface MangaBakaStatus {
  connected: boolean;
  username: string | null;
  connectedAt: string | null;
}

/** Always "not connected"; no network call. */
export async function getMangaBakaStatus(): Promise<MangaBakaStatus> {
  return { connected: false, username: null, connectedAt: null };
}

export async function disconnectMangaBaka(): Promise<void> {
  /* nothing to disconnect */
}

export interface MangaBakaLibraryEntry {
  seriesId: number;
  state: string | null;
  progressChapter: number | null;
  progressVolume: number | null;
  rating: number | null;
  title: string | null;
  coverUrl: string | null;
  anilistId: number | null;
  malId: number | null;
  kitsuId: number | null;
  updatedAt: string | null;
}

/** MangaBaka library is unavailable: always empty, no network call. */
export async function fetchMangaBakaLibrary(): Promise<MangaBakaLibraryEntry[]> {
  return [];
}

const MANGABAKA_TO_TATAKAI: Record<string, string> = {
  reading: 'reading',
  rereading: 'reading',
  completed: 'completed',
  plan_to_read: 'plan_to_read',
  considering: 'plan_to_read',
  dropped: 'dropped',
  paused: 'on_hold',
};

export function mapMangaBakaStatusToTatakai(status: string): string {
  return MANGABAKA_TO_TATAKAI[String(status || '').trim().toLowerCase()] || 'plan_to_read';
}

/** No-op in this fork (MangaBaka is disabled): nothing is sent. */
export async function pushMangaProgressViaServer(_input: {
  anilistId?: number | null;
  malId?: number | null;
  status: string;
  chapter?: number | null;
}): Promise<boolean> {
  return false;
}
