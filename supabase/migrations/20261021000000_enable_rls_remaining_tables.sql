-- Fork hardening: these public tables were created without RLS, so the anon
-- key could read and write them through the Data API. None is queried by the
-- client directly.
--   * anime_trending_scores is read by get_trending_scores() (SECURITY
--     INVOKER), so it keeps public read access; writes go through
--     refresh_trending_scores() (SECURITY DEFINER), which bypasses RLS.
--   * The rest are backend-only (service_role bypasses RLS): no policies.
ALTER TABLE public.anime_trending_scores ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS "Anyone can read trending scores" ON public.anime_trending_scores;
CREATE POLICY "Anyone can read trending scores"
  ON public.anime_trending_scores FOR SELECT
  TO anon, authenticated
  USING (true);

ALTER TABLE public.api_job_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.api_manga_home_daily ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.api_source_validation_queue ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.country_policy_audit_log ENABLE ROW LEVEL SECURITY;
