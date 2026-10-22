-- =============================================================================
-- AniList / MyAnimeList tokens: out of `profiles`, into an owner-only table
--
-- `profiles` has the policy "Profiles are viewable by everyone" (USING true) and
-- the anon role can SELECT every column, so the OAuth tokens that
-- 20260902000003 let the client store there (mal_access_token,
-- mal_refresh_token, anilist_access_token, ...) were readable by anyone holding
-- the public anon key.
--
-- This migration:
--   1. creates public.user_integration_tokens (RLS, owner-only, no anon access)
--   2. copies existing token values into it
--   3. NULLs the token columns on profiles (their values were exposed, so
--      users re-link if they want a fresh token) and revokes client UPDATE on
--      them. The non-secret anilist_user_id / mal_user_id stay on profiles and
--      remain the "is linked" signal.
--
-- Idempotent: safe to run more than once.
-- =============================================================================

CREATE TABLE IF NOT EXISTS public.user_integration_tokens (
  user_id uuid PRIMARY KEY REFERENCES auth.users(id) ON DELETE CASCADE,
  anilist_access_token text,
  anilist_token_expires_at timestamptz,
  mal_access_token text,
  mal_refresh_token text,
  mal_token_expires_at timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE public.user_integration_tokens ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users read own integration tokens" ON public.user_integration_tokens;
CREATE POLICY "Users read own integration tokens"
  ON public.user_integration_tokens FOR SELECT
  TO authenticated
  USING (auth.uid() = user_id);

DROP POLICY IF EXISTS "Users insert own integration tokens" ON public.user_integration_tokens;
CREATE POLICY "Users insert own integration tokens"
  ON public.user_integration_tokens FOR INSERT
  TO authenticated
  WITH CHECK (auth.uid() = user_id);

DROP POLICY IF EXISTS "Users update own integration tokens" ON public.user_integration_tokens;
CREATE POLICY "Users update own integration tokens"
  ON public.user_integration_tokens FOR UPDATE
  TO authenticated
  USING (auth.uid() = user_id)
  WITH CHECK (auth.uid() = user_id);

DROP POLICY IF EXISTS "Users delete own integration tokens" ON public.user_integration_tokens;
CREATE POLICY "Users delete own integration tokens"
  ON public.user_integration_tokens FOR DELETE
  TO authenticated
  USING (auth.uid() = user_id);

REVOKE ALL ON public.user_integration_tokens FROM PUBLIC;
REVOKE ALL ON public.user_integration_tokens FROM anon;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.user_integration_tokens TO authenticated;
GRANT ALL ON public.user_integration_tokens TO service_role;

-- -----------------------------------------------------------------------------
-- Move existing values, then scrub profiles
-- -----------------------------------------------------------------------------
-- Every column is (re)declared first so the statements below cannot fail on a
-- database where an older migration never created one of them.
ALTER TABLE public.profiles
  ADD COLUMN IF NOT EXISTS anilist_access_token text,
  ADD COLUMN IF NOT EXISTS anilist_refresh_token text,
  ADD COLUMN IF NOT EXISTS anilist_token_expires_at timestamptz,
  ADD COLUMN IF NOT EXISTS mal_access_token text,
  ADD COLUMN IF NOT EXISTS mal_refresh_token text,
  ADD COLUMN IF NOT EXISTS mal_token_expires_at timestamptz;

INSERT INTO public.user_integration_tokens (
  user_id,
  anilist_access_token, anilist_token_expires_at,
  mal_access_token, mal_refresh_token, mal_token_expires_at
)
SELECT
  p.user_id,
  p.anilist_access_token, p.anilist_token_expires_at,
  p.mal_access_token, p.mal_refresh_token, p.mal_token_expires_at
FROM public.profiles p
JOIN auth.users u ON u.id = p.user_id
WHERE p.anilist_access_token IS NOT NULL
   OR p.mal_access_token IS NOT NULL
   OR p.mal_refresh_token IS NOT NULL
ON CONFLICT (user_id) DO UPDATE SET
  anilist_access_token = COALESCE(public.user_integration_tokens.anilist_access_token, EXCLUDED.anilist_access_token),
  anilist_token_expires_at = COALESCE(public.user_integration_tokens.anilist_token_expires_at, EXCLUDED.anilist_token_expires_at),
  mal_access_token = COALESCE(public.user_integration_tokens.mal_access_token, EXCLUDED.mal_access_token),
  mal_refresh_token = COALESCE(public.user_integration_tokens.mal_refresh_token, EXCLUDED.mal_refresh_token),
  mal_token_expires_at = COALESCE(public.user_integration_tokens.mal_token_expires_at, EXCLUDED.mal_token_expires_at),
  updated_at = now();

UPDATE public.profiles
SET anilist_access_token = NULL,
    anilist_refresh_token = NULL,
    anilist_token_expires_at = NULL,
    mal_access_token = NULL,
    mal_refresh_token = NULL,
    mal_token_expires_at = NULL
WHERE anilist_access_token IS NOT NULL
   OR anilist_refresh_token IS NOT NULL
   OR anilist_token_expires_at IS NOT NULL
   OR mal_access_token IS NOT NULL
   OR mal_refresh_token IS NOT NULL
   OR mal_token_expires_at IS NOT NULL;

-- Nothing on the client may write tokens to profiles any more (the column-level
-- grant came from 20260902000003). mal_user_id / anilist_user_id stay writable.
REVOKE UPDATE (
  anilist_access_token,
  anilist_refresh_token,
  anilist_token_expires_at,
  mal_access_token,
  mal_refresh_token,
  mal_token_expires_at
) ON public.profiles FROM authenticated, anon;

-- Belt and braces: whatever role or legacy code path still writes the old
-- columns (edge function, SECURITY DEFINER helper, a stale client build), the
-- value is dropped instead of being stored in a world-readable row.
CREATE OR REPLACE FUNCTION public.strip_profile_integration_tokens()
RETURNS trigger
LANGUAGE plpgsql
AS $fn$
BEGIN
  NEW.anilist_access_token := NULL;
  NEW.anilist_refresh_token := NULL;
  NEW.anilist_token_expires_at := NULL;
  NEW.mal_access_token := NULL;
  NEW.mal_refresh_token := NULL;
  NEW.mal_token_expires_at := NULL;
  RETURN NEW;
END;
$fn$;

DROP TRIGGER IF EXISTS profiles_strip_integration_tokens ON public.profiles;
CREATE TRIGGER profiles_strip_integration_tokens
  BEFORE INSERT OR UPDATE ON public.profiles
  FOR EACH ROW EXECUTE FUNCTION public.strip_profile_integration_tokens();
