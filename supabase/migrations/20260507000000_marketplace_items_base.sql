-- Fork fix: public.marketplace_items exists on the live upstream schema
-- (main.sql) but no migration creates it, so 20260508000002 aborts with 42P01
-- on a fresh project. Create the legacy base table here; 20260508000002 adds the
-- typed columns, check constraint, indexes and RLS policies.
CREATE TABLE IF NOT EXISTS public.marketplace_items (
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid,
  type text NOT NULL,
  anime_id text NOT NULL,
  anime_name text,
  episode_number integer,
  data jsonb NOT NULL,
  status text DEFAULT 'pending'::text,
  moderator_id uuid,
  created_at timestamp with time zone DEFAULT now(),
  CONSTRAINT marketplace_items_pkey PRIMARY KEY (id),
  CONSTRAINT marketplace_items_moderator_id_fkey FOREIGN KEY (moderator_id) REFERENCES auth.users(id),
  CONSTRAINT marketplace_items_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.profiles(user_id)
);

ALTER TABLE public.marketplace_items ENABLE ROW LEVEL SECURITY;
