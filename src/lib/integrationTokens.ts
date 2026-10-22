/**
 * Storage for the user's AniList / MyAnimeList OAuth tokens.
 *
 * The tokens live in `public.user_integration_tokens` (RLS: owner only, no anon
 * access). They must never be written to `profiles`, which is publicly readable.
 * Only the non-secret account ids (`anilist_user_id`, `mal_user_id`) stay on
 * `profiles`, and double as the "is linked" signal.
 */

export interface IntegrationTokenRow {
  anilist_access_token: string | null;
  anilist_token_expires_at: string | null;
  mal_access_token: string | null;
  mal_refresh_token: string | null;
  mal_token_expires_at: string | null;
}

export type IntegrationTokenPatch = Partial<IntegrationTokenRow>;
export type ProfileIdPatch = { anilist_user_id?: string | null; mal_user_id?: string | null };

async function session() {
  const { supabase } = await import('@/integrations/supabase/client');
  const { data: { session } } = await supabase.auth.getSession();
  if (!session?.user) throw new Error('Sign in to Tatakai first.');
  return { supabase: supabase as any, userId: session.user.id as string };
}

/** The signed-in user's own token row, or null when nothing is stored. */
export async function loadIntegrationTokens(): Promise<IntegrationTokenRow | null> {
  const { supabase, userId } = await session();
  const { data, error } = await supabase
    .from('user_integration_tokens')
    .select('anilist_access_token, anilist_token_expires_at, mal_access_token, mal_refresh_token, mal_token_expires_at')
    .eq('user_id', userId)
    .maybeSingle();
  if (error) throw error;
  return (data as IntegrationTokenRow | null) ?? null;
}

/** Write token columns to the owner-only table (partial upsert). */
export async function saveIntegrationTokens(patch: IntegrationTokenPatch): Promise<void> {
  const { supabase, userId } = await session();
  const { error } = await supabase
    .from('user_integration_tokens')
    .upsert({ user_id: userId, ...patch, updated_at: new Date().toISOString() }, { onConflict: 'user_id' });
  if (error) throw error;
}

/** Write the non-secret account ids on the user's profile row. */
export async function saveProfileIds(patch: ProfileIdPatch): Promise<void> {
  const { supabase, userId } = await session();
  const { error } = await supabase.from('profiles').update(patch).eq('user_id', userId);
  if (error) throw error;
}

const TOKEN_KEYS = [
  'anilist_access_token', 'anilist_token_expires_at',
  'mal_access_token', 'mal_refresh_token', 'mal_token_expires_at',
] as const;
const ID_KEYS = ['anilist_user_id', 'mal_user_id'] as const;

/** Route a mixed patch: tokens to the private table, ids to profiles. */
export async function saveIntegrationPatch(patch: IntegrationTokenPatch & ProfileIdPatch): Promise<void> {
  const tokens: Record<string, unknown> = {};
  const ids: Record<string, unknown> = {};
  for (const key of TOKEN_KEYS) if (key in patch) tokens[key] = (patch as Record<string, unknown>)[key];
  for (const key of ID_KEYS) if (key in patch) ids[key] = (patch as Record<string, unknown>)[key];
  if (Object.keys(tokens).length) await saveIntegrationTokens(tokens as IntegrationTokenPatch);
  if (Object.keys(ids).length) await saveProfileIds(ids as ProfileIdPatch);
}

/** The user's AniList token + AniList account id (profiles), or null when not linked. */
export async function loadAniListCredentials(): Promise<{ token: string; userId: number } | null> {
  const { supabase, userId } = await session();
  const [tokens, profile] = await Promise.all([
    loadIntegrationTokens(),
    supabase.from('profiles').select('anilist_user_id').eq('user_id', userId).maybeSingle(),
  ]);
  const aniUser = Number(profile?.data?.anilist_user_id);
  if (!tokens?.anilist_access_token || !Number.isFinite(aniUser)) return null;
  return { token: tokens.anilist_access_token, userId: aniUser };
}
