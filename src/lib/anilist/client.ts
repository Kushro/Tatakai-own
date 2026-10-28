/**
 * AniList GraphQL client — talks to https://graphql.anilist.co directly
 * (no TatakaiAPI proxy). Paced and retried through publicApi/hostLimiter.
 *
 * Use `anilistGql` for every AniList call. Pass `token` only for calls that act
 * on the signed-in user's AniList account (Viewer, MediaList mutations).
 */

import { publicJson, PublicApiError } from '@/lib/net/publicApi';

export const ANILIST_URL = 'https://graphql.anilist.co';

export class AniListError extends Error {
  constructor(message: string, readonly status: number, readonly errors: Array<{ message?: string; status?: number }> = []) {
    super(message);
    this.name = 'AniListError';
  }
  /** AniList answers 404 (HTTP or per-error status) for unknown ids. */
  get isNotFound(): boolean {
    return this.status === 404 || this.errors.some((e) => e.status === 404);
  }
}

export interface AniListGqlOptions {
  /** User AniList OAuth access token (only for user-scoped queries/mutations). */
  token?: string | null;
  signal?: AbortSignal;
  retries?: number;
}

interface GqlEnvelope<T> {
  data?: T | null;
  errors?: Array<{ message?: string; status?: number }>;
}

/**
 * Run a GraphQL query/mutation against AniList and return `data`.
 * Throws AniListError on HTTP failure or when AniList returns errors with no data.
 * When AniList returns partial data plus errors, the partial data is returned.
 */
export async function anilistGql<T>(
  query: string,
  variables?: Record<string, unknown>,
  options: AniListGqlOptions = {},
): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (options.token) headers.Authorization = `Bearer ${options.token}`;
  let env: GqlEnvelope<T>;
  try {
    env = await publicJson<GqlEnvelope<T>>(ANILIST_URL, {
      method: 'POST',
      headers,
      body: JSON.stringify(variables ? { query, variables } : { query }),
      signal: options.signal,
      retries: options.retries,
    });
  } catch (err) {
    if (err instanceof PublicApiError) {
      let errors: GqlEnvelope<T>['errors'] = [];
      try { errors = (JSON.parse(err.body || '{}') as GqlEnvelope<T>).errors || []; } catch { /* non-JSON body */ }
      throw new AniListError(errors[0]?.message || err.message, err.status, errors);
    }
    throw err;
  }
  if (env.errors?.length && env.data == null) {
    throw new AniListError(env.errors[0].message || 'AniList GraphQL error', env.errors[0].status ?? 400, env.errors);
  }
  return env.data as T;
}
