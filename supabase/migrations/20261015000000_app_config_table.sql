-- Fork fix: public.app_config exists on the live upstream schema (main.sql) but
-- no migration creates it, so 20261015010000 aborts with 42P01 on a fresh
-- project. Same columns as main.sql, plus RLS: everyone may read the flags
-- (useAppConfig, useForum), only admins may change them.
CREATE TABLE IF NOT EXISTS public.app_config (
  key text NOT NULL,
  value text,
  CONSTRAINT app_config_pkey PRIMARY KEY (key)
);

ALTER TABLE public.app_config ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Anyone can read app config" ON public.app_config;
CREATE POLICY "Anyone can read app config"
  ON public.app_config FOR SELECT
  TO anon, authenticated
  USING (true);

DROP POLICY IF EXISTS "Admins can manage app config" ON public.app_config;
CREATE POLICY "Admins can manage app config"
  ON public.app_config FOR ALL
  TO authenticated
  USING (EXISTS (SELECT 1 FROM public.profiles
                 WHERE user_id = auth.uid() AND (is_admin OR role = 'admin')))
  WITH CHECK (EXISTS (SELECT 1 FROM public.profiles
                      WHERE user_id = auth.uid() AND (is_admin OR role = 'admin')));

GRANT SELECT ON public.app_config TO anon, authenticated;
GRANT INSERT, UPDATE, DELETE ON public.app_config TO authenticated;
