/**
 * hostLimiter.ts — client-side request pacing per public API host.
 *
 * TatakaiAPI used to absorb AniList/Jikan rate limits server-side. Now the app
 * talks to those APIs directly, so every request is scheduled through a
 * per-host queue that enforces a minimum spacing and a rolling per-minute cap,
 * and can be paused when a server answers 429 with Retry-After.
 */

export interface HostLimit {
  /** Minimum milliseconds between two request starts. */
  minIntervalMs: number;
  /** Maximum request starts in any rolling 60 s window. */
  perMinute: number;
}

/** Published limits (with headroom) for the hosts the app calls directly. */
export const HOST_LIMITS: Record<string, HostLimit> = {
  // AniList: 90/min nominal, sometimes degraded to 30/min. The limit is
  // tightened at runtime from X-RateLimit-Limit (see adoptServerLimit).
  'graphql.anilist.co': { minIntervalMs: 250, perMinute: 80 },
  // Tenrai (Jikan successor) publishes no numbers; stay at Jikan's old pace.
  'api.tenrai.org': { minIntervalMs: 300, perMinute: 100 },
  // Shikimori: 5 req/s and 90 req/min.
  'shikimori.io': { minIntervalMs: 220, perMinute: 80 },
  'api.mangadex.org': { minIntervalMs: 220, perMinute: 200 },
  'api.myanimelist.net': { minIntervalMs: 350, perMinute: 120 },
  'myanimelist.net': { minIntervalMs: 500, perMinute: 30 },
  // ani.zip publishes no limit and serves cached JSON; list pages resolve many
  // ids at once, so allow a quicker pace.
  'api.ani.zip': { minIntervalMs: 60, perMinute: 400 },
  'kitsu.io': { minIntervalMs: 250, perMinute: 120 },
  'api.mangaupdates.com': { minIntervalMs: 600, perMinute: 60 },
};

const DEFAULT_LIMIT: HostLimit = { minIntervalMs: 200, perMinute: 120 };

interface HostState {
  limit: HostLimit;
  starts: number[];
  lastStart: number;
  pausedUntil: number;
  chain: Promise<void>;
}

const states = new Map<string, HostState>();

function stateFor(host: string): HostState {
  let s = states.get(host);
  if (!s) {
    s = { limit: { ...(HOST_LIMITS[host] ?? DEFAULT_LIMIT) }, starts: [], lastStart: 0, pausedUntil: 0, chain: Promise.resolve() };
    states.set(host, s);
  }
  return s;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/** Milliseconds to wait before `host` may start another request. */
function waitFor(s: HostState, now: number): number {
  s.starts = s.starts.filter((t) => now - t < 60_000);
  let wait = Math.max(0, s.pausedUntil - now, s.lastStart + s.limit.minIntervalMs - now);
  if (s.starts.length >= s.limit.perMinute) wait = Math.max(wait, s.starts[0] + 60_000 - now);
  return wait;
}

/**
 * Resolve when `host` may start a request; the caller should start it right
 * away. Callers are served in FIFO order per host.
 */
export function acquireSlot(host: string, signal?: AbortSignal): Promise<void> {
  if (pacingDisabled) return Promise.resolve();
  const s = stateFor(host);
  const turn = s.chain.then(async () => {
    for (;;) {
      signal?.throwIfAborted();
      const wait = waitFor(s, Date.now());
      if (wait <= 0) break;
      await sleep(Math.min(wait, 5_000));
    }
    const now = Date.now();
    s.lastStart = now;
    s.starts.push(now);
  });
  // Keep the chain alive even if this caller aborts.
  s.chain = turn.catch(() => undefined);
  return turn;
}

/** Stop starting requests to `host` for `ms` (e.g. from a 429 Retry-After). */
export function pauseHost(host: string, ms: number): void {
  const s = stateFor(host);
  s.pausedUntil = Math.max(s.pausedUntil, Date.now() + Math.max(0, ms));
}

/** Tighten the per-minute cap when the server advertises a lower limit. */
export function adoptServerLimit(host: string, perMinute: number | null | undefined): void {
  if (!perMinute || !Number.isFinite(perMinute) || perMinute <= 0) return;
  const s = stateFor(host);
  const capped = Math.max(1, Math.floor(perMinute * 0.9));
  if (capped < s.limit.perMinute) s.limit.perMinute = capped;
}

/** Test hook: forget all pacing state. */
export function resetHostLimiter(): void {
  states.clear();
}

let pacingDisabled = false;
/** Test hook: skip pacing entirely (unit tests with stubbed fetch). */
export function setHostPacingDisabled(disabled: boolean): void {
  pacingDisabled = disabled;
}
export function isHostPacingDisabled(): boolean {
  return pacingDisabled;
}
