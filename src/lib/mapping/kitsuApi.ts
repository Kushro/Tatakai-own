/**
 * Minimal Kitsu JSON:API transport, shared by the chapter-hierarchy mapping
 * (`./kitsu.ts`), the Kitsu metadata provider and the MangaDex crosswalk.
 *
 * Verified live against https://kitsu.io/api/edge:
 *  - `page[limit]` is capped at 20 everywhere (400 above that).
 *  - `filter[nsfw]` / `filter[ageRating]` on manga are not allowed filters.
 *  - Mapping lookups are `/mappings?filter[externalSite]=anilist/manga&
 *    filter[externalId]=N&include=item` (item id = Kitsu id).
 */

import { publicJson, PublicApiError } from "@/lib/net/publicApi";
import { loadMapping } from "./client";

export const KITSU_BASE = "https://kitsu.io/api/edge";
export const KITSU_PAGE_MAX = 20;

export type KitsuKind = "anime" | "manga";

export interface KitsuRelRef { id: string; type: string }

export interface KitsuResource {
  id: string;
  type: string;
  attributes: Record<string, any>;
  relationships?: Record<string, { data?: KitsuRelRef | KitsuRelRef[] | null }>;
}

export interface KitsuDoc {
  data: KitsuResource | KitsuResource[];
  included?: KitsuResource[];
  meta?: { count?: number };
  links?: { next?: string };
}

export type KitsuParams = Record<string, string | number | boolean | undefined | null>;

/** GET a Kitsu resource/collection. Params keep their JSON:API keys (`page[limit]`). */
export async function kitsuGet(path: string, params: KitsuParams = {}, signal?: AbortSignal): Promise<KitsuDoc> {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== "") q.set(k, String(v));
  }
  const qs = q.toString();
  return publicJson<KitsuDoc>(`${KITSU_BASE}${path}${qs ? `?${qs}` : ""}`, {
    headers: { Accept: "application/vnd.api+json" },
    signal,
  });
}

/** Rows of a collection document (always an array). */
export function kitsuRows(doc: KitsuDoc): KitsuResource[] {
  return Array.isArray(doc.data) ? doc.data : doc.data ? [doc.data] : [];
}

/** Included resources referenced by `res.relationships[rel]`, in relationship order. */
export function kitsuRelated(doc: KitsuDoc, res: KitsuResource, rel: string): KitsuResource[] {
  const data = res.relationships?.[rel]?.data;
  if (!data || !doc.included) return [];
  const refs = Array.isArray(data) ? data : [data];
  const index = new Map(doc.included.map((r) => [`${r.type}:${r.id}`, r]));
  const out: KitsuResource[] = [];
  for (const ref of refs) {
    const hit = index.get(`${ref.type}:${ref.id}`);
    if (hit) out.push(hit);
  }
  return out;
}

export interface KitsuExternalIds {
  anilistId?: number;
  malId?: number;
}

/** Pull AniList / MAL ids out of a title's included `mappings` for `kind`. */
export function externalIdsOf(mappings: KitsuResource[], kind: KitsuKind): KitsuExternalIds {
  const out: KitsuExternalIds = {};
  for (const m of mappings) {
    const site = m.attributes.externalSite;
    const id = Number(m.attributes.externalId);
    if (!Number.isInteger(id) || id <= 0) continue;
    if (site === `anilist/${kind}`) out.anilistId = id;
    else if (site === `myanimelist/${kind}`) out.malId = id;
  }
  return out;
}

export interface KitsuLookup {
  kitsuId: number;
  /** Distinct titles (canonical, en, romaji, ja) for title-based crosswalks. */
  titles: string[];
}

const LOOKUP_TTL_MS = 24 * 60 * 60 * 1000;

/**
 * Resolve a Kitsu id from an AniList / MAL id through Kitsu's mappings. Null when
 * Kitsu has no mapping; throws on transport failure.
 */
export async function lookupKitsuByExternal(
  kind: KitsuKind,
  site: "anilist" | "myanimelist",
  externalId: number,
  signal?: AbortSignal,
): Promise<KitsuLookup | null> {
  if (!Number.isInteger(externalId) || externalId <= 0) return null;
  return loadMapping<KitsuLookup>(`kitsu:lookup:${kind}:${site}:${externalId}`, async (sig) => {
    const doc = await kitsuGet("/mappings", {
      "filter[externalSite]": `${site}/${kind}`,
      "filter[externalId]": externalId,
      include: "item",
      "page[limit]": 1,
    }, sig);
    const item = doc.included?.find((r) => r.type === kind);
    if (!item) return null;
    const a = item.attributes;
    const titles = [a.canonicalTitle, a.titles?.en, a.titles?.en_jp, a.titles?.en_us, a.titles?.ja_jp]
      .filter((t): t is string => typeof t === "string" && t.trim() !== "");
    return { kitsuId: Number(item.id), titles: [...new Set(titles)] };
  }, { ttlMs: LOOKUP_TTL_MS, signal, rethrow: true });
}

/** True for HTTP 404/400 from Kitsu (unknown id / unsupported request). */
export function isKitsuMiss(err: unknown): boolean {
  return err instanceof PublicApiError && (err.status === 404 || err.status === 400);
}
