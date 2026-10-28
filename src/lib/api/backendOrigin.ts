/**
 * backendOrigin.ts — legacy resolver for the upstream TatakaiAPI backend.
 *
 * This fork no longer depends on TatakaiAPI: catalog data comes from the
 * metadata router (`@/lib/metadata`), accounts/sync talk to AniList/MAL and
 * Supabase directly. The resolver is kept only so old imports keep compiling,
 * and it fails closed: unless a backend is explicitly configured at build time
 * (`VITE_BACKEND_ORIGIN` / `VITE_TATAKAI_API_URL`), it returns "" and never
 * falls back to api.tatakai.me or the page origin.
 */

/** Explicitly configured backend origin without trailing slash, or "". */
export function resolveBackendOrigin(): string {
  const explicit = String(import.meta.env.VITE_BACKEND_ORIGIN || "").trim();
  if (/^https?:\/\//i.test(explicit)) return explicit.replace(/\/+$/, "");

  const apiUrl = String(import.meta.env.VITE_TATAKAI_API_URL || "").trim();
  try {
    if (apiUrl) return new URL(apiUrl).origin;
  } catch {
    /* fall through */
  }
  return "";
}

/** `${origin}/api/v3` for an explicitly configured backend, or "". */
export function resolveApiV3Base(): string {
  const origin = resolveBackendOrigin();
  return origin ? `${origin}/api/v3` : "";
}

/**
 * AniList GraphQL endpoint. This fork calls AniList directly instead of the
 * TatakaiAPI proxy; prefer `anilistGql` from `@/lib/anilist/client`, which also
 * paces requests and handles 429s.
 */
export { ANILIST_URL as ANILIST_GRAPHQL_ENDPOINT } from "@/lib/anilist/client";
