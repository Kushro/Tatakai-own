-- Fork fix: columns present on the live upstream schema (main.sql) that no
-- migration adds, but that later migrations reference (e.g. the column-level
-- GRANT on watch_rooms in 20260924000001). Same definitions as main.sql.
ALTER TABLE public.watch_rooms
  ADD COLUMN IF NOT EXISTS manual_stream_url text,
  ADD COLUMN IF NOT EXISTS manual_stream_type text DEFAULT 'direct'::text,
  ADD COLUMN IF NOT EXISTS selected_server text;

ALTER TABLE public.forum_posts
  ADD COLUMN IF NOT EXISTS image_url text,
  ADD COLUMN IF NOT EXISTS is_approved boolean DEFAULT true;
