-- Token sheets, cadence, approval and reject (docs: Account > Cadence and
-- token sheets, Approval; Attribute > Review and submit).

-- How often token sheets are due, for the whole organization (no project)
-- or for one project. Weekly, every two weeks and custom cadences are runs
-- of days counted from `starts_on`; monthly follows calendar months.
CREATE TABLE cadences (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    project_id uuid REFERENCES projects (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('weekly', 'every_two_weeks', 'monthly', 'custom')),
    every_days integer CHECK (every_days BETWEEN 1 AND 366),
    CHECK ((kind = 'custom') = (every_days IS NOT NULL)),
    starts_on date NOT NULL,
    set_by uuid NOT NULL REFERENCES people (id),
    set_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE NULLS NOT DISTINCT (organization_id, project_id)
);

-- One agent manager's submitted token entries for one cadence period.
CREATE TABLE token_sheets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    person_id uuid NOT NULL REFERENCES people (id),
    starts_on date NOT NULL,
    ends_on date NOT NULL CHECK (ends_on >= starts_on),
    first_submitted_at timestamptz NOT NULL DEFAULT now(),
    last_submitted_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, person_id, starts_on)
);

-- Which token sheet a token entry came in on. Automatic token entries are on
-- none.
CREATE TABLE token_sheet_entries (
    token_entry_id uuid PRIMARY KEY REFERENCES token_entries (id) ON DELETE CASCADE,
    token_sheet_id uuid NOT NULL REFERENCES token_sheets (id) ON DELETE CASCADE
);

CREATE INDEX token_sheet_entries_by_sheet ON token_sheet_entries (token_sheet_id);

-- Every approval and every reject, newest last. A token entry's status says
-- where it stands; this says who decided and why.
CREATE TABLE token_entry_decisions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    token_entry_id uuid NOT NULL REFERENCES token_entries (id) ON DELETE CASCADE,
    decision text NOT NULL CHECK (decision IN ('approved', 'rejected')),
    reason text,
    CHECK (decision <> 'rejected' OR length(btrim(coalesce(reason, ''))) > 0),
    decided_by uuid NOT NULL REFERENCES people (id),
    decided_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX token_entry_decisions_by_entry ON token_entry_decisions (token_entry_id, decided_at DESC);
