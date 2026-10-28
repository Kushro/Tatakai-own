-- Fork fix: policies from 20260228000001 onward check profiles.role, but the
-- column is only added in 20260511000001_add_missing_analytics_columns.sql
-- (IF NOT EXISTS), so a from-scratch `db push` aborts with 42703. Add it early
-- with the same definition the live schema has.
ALTER TABLE public.profiles ADD COLUMN IF NOT EXISTS role TEXT DEFAULT 'user';
