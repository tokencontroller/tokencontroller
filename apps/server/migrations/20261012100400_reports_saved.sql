-- Saved reports: a tag filter with a grouping (docs: Report > Reading
-- reports).
CREATE TABLE saved_reports (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(btrim(name)) > 0),
    filter jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(filter) = 'object'),
    group_by text NOT NULL CHECK (length(group_by) > 0),
    created_by uuid NOT NULL REFERENCES people (id),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX saved_reports_by_organization ON saved_reports (organization_id);
