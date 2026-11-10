-- second step: no released version reads or writes people.role since public #127 (production hosted-2026-11-10-59)
-- A person's role is read only from the grants their role wrote in the
-- event log (grants::role_of). The column that also held it is gone, so
-- the two can no longer disagree.
ALTER TABLE people DROP COLUMN role;
