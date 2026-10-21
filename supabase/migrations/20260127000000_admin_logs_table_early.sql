-- Fork fix: 20260128000002_moderator_suggestions_rls.sql creates policies ON
-- public.admin_logs, but the table is only created in
-- 20260902000004_admin_logs.sql, so a from-scratch `db push` aborts with 42P01.
-- Create the bare table (same columns) early; the September migration is
-- IF NOT EXISTS and still adds its indexes, policies and grants.
CREATE TABLE IF NOT EXISTS public.admin_logs (
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid,
  action text NOT NULL,
  entity_type text NOT NULL,
  entity_id text,
  details jsonb,
  ip_address text,
  created_at timestamp with time zone DEFAULT now(),
  CONSTRAINT admin_logs_pkey PRIMARY KEY (id),
  CONSTRAINT admin_logs_user_id_fkey FOREIGN KEY (user_id)
    REFERENCES auth.users(id) ON DELETE SET NULL
);

ALTER TABLE public.admin_logs ENABLE ROW LEVEL SECURITY;
