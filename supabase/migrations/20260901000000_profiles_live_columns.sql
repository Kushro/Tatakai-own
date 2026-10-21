-- Fork fix: these profiles columns exist on the live upstream schema (main.sql)
-- and are referenced by later migrations (e.g. the column-level GRANT in
-- 20260902000003) and the client, but no migration adds them. Add them with the
-- live definitions so a from-scratch `db push` succeeds.
ALTER TABLE public.profiles
  ADD COLUMN IF NOT EXISTS last_seen timestamp with time zone DEFAULT now(),
  ADD COLUMN IF NOT EXISTS mal_auto_delete boolean DEFAULT false,
  ADD COLUMN IF NOT EXISTS anilist_refresh_token text,
  ADD COLUMN IF NOT EXISTS device_id text;
