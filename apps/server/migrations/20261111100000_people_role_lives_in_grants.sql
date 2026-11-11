-- second step: no released version writes or reads people.role since public #134 (production hosted-2026-11-11-70)
-- A person's role is read only from the grants their role wrote in the
-- event log (grants::role_of). The column that also held it is gone, so
-- the two can no longer disagree.
ALTER TABLE people DROP COLUMN role;
