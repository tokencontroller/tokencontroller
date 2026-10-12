-- Tag names and the tag map. Tag values live as JSON objects on the records
-- that carry them (people, projects, tickets, splits). Docs: Account > Tags
-- and the tag map.

-- The organization's list of tag names. `person`, `ticket` and `project`
-- are built in and always present.
CREATE TABLE tag_names (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[a-z][a-z0-9_-]*$' AND length(name) <= 40),
    built_in boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, name)
);

-- Every version of the tag map, numbered from 1 per organization. The
-- highest is in force; a closed period names the version it closed with
-- (ADR 0007), so rows are never changed.
CREATE TABLE tag_map_versions (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    version integer NOT NULL CHECK (version > 0),
    -- The tag name that fills the cost center. Default `person`.
    cost_center text NOT NULL DEFAULT 'person',
    -- Tag names that fill the cost object, most specific first. The
    -- organization is always the last rung and is not listed.
    ladder text[] NOT NULL DEFAULT '{ticket,project}',
    created_by uuid REFERENCES people (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, version)
);
