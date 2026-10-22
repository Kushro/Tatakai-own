/**
 * MangaBaka mapping — DISABLED in this fork.
 *
 * MangaBaka was only reachable through the TatakaiAPI `/mapping/*` proxy, which
 * the fork no longer talks to, and there is no keyless public API to replace it.
 * The types below are kept (consumers import them) and every function resolves to
 * `null` without touching the network, which callers already treat as "no extra
 * detail". Series metadata comes from the metadata providers instead
 * (`@/lib/metadata`); the Kitsu chapter hierarchy no longer needs MangaBaka
 * (see `./kitsu.ts`).
 */

// ── Normalized shapes ─────────────────────────────────────────────────────────

export interface MangaBakaTag {
  id: number | null;
  name: string;
  /** `"Activities > Conflict > Duels"` — the tag's place in MangaBaka's tree. */
  path: string | null;
  /** `core` | `defining` | `incidental` — how central the tag is to the series. */
  weight: string | null;
  isSpoiler: boolean;
}

export interface MangaBakaPublisher {
  name: string;
  /** `Original` | `English` | `Other` — the territory this publisher covers. */
  type: string | null;
  note: string | null;
}

export interface MangaBakaLink {
  url: string;
  label: string;
  type: string | null;
  language: string | null;
}

export interface MangaBakaSourceId {
  id: string;
  rating: number | null;
  /** Rating rescaled to 0–100 so sources with different scales are comparable. */
  ratingNormalized: number | null;
}

export interface MangaBakaCover {
  /** Full-resolution original. Extensionless — do not append a file suffix. */
  original: string | null;
  small: string | null;
  medium: string | null;
  large: string | null;
  width: number | null;
  height: number | null;
  blurhash: string | null;
}

export interface MangaBakaSeries {
  mangaBakaId: number | null;
  title: string;
  nativeTitle: string | null;
  romanizedTitle: string | null;
  /** Distinct titles across languages, best-first. Feeds alias search. */
  aliases: string[];
  /** Localized titles with provenance, as MangaBaka records them. */
  localizedTitles: Array<{ language: string; title: string; traits: string[]; isPrimary: boolean }>;
  description: string | null;
  cover: MangaBakaCover;
  authors: string[];
  artists: string[];
  publishers: MangaBakaPublisher[];
  /** Human-readable genre labels, from `genres_v2` or the v1 slug list. */
  genres: string[];
  tags: MangaBakaTag[];
  status: string | null;
  contentRating: string | null;
  type: string | null;
  year: number | null;
  publishedStart: string | null;
  publishedEnd: string | null;
  /** 0–100. */
  rating: number | null;
  popularityRank: number | null;
  totalChapters: number | null;
  totalVolumes: number | null;
  isLicensed: boolean;
  hasAnime: boolean;
  /** Where an anime adaptation starts and stops in the manga, in prose. */
  animeCoverage: { start: string | null; end: string | null } | null;
  links: MangaBakaLink[];
  /** Per-tracker ids and ratings, keyed `anilist`, `kitsu`, `my_anime_list`, … */
  sources: Record<string, MangaBakaSourceId>;
  /** Convenience accessor — the id the chapter hierarchy is built from. */
  kitsuId: number | null;
  malId: number | null;
  anilistId: number | null;
  lastUpdatedAt: string | null;
}

// ── Fetching (disabled) ───────────────────────────────────────────────────────

/**
 * Sources MangaBaka can be queried by. Kept for API compatibility.
 */
export type MangaBakaSource =
  | "mangabaka"
  | "anilist"
  | "my_anime_list"
  | "kitsu"
  | "manga_updates"
  | "anime_planet";

/** MangaBaka is disabled: always resolves to `null`, never touches the network. */
export async function fetchMangaBakaSeries(
  _sourceId: number | string | null | undefined,
  _options: { source?: MangaBakaSource; signal?: AbortSignal; force?: boolean } = {},
): Promise<MangaBakaSeries | null> {
  return null;
}

/** No-op: there is nothing cached. */
export function clearMangaBakaCache(_anilistId?: number): void {
  /* MangaBaka is disabled */
}
