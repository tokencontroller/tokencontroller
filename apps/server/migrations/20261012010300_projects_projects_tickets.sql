-- Projects with their repositories, and tickets: the built-in tags with
-- records behind them. A client is a tag on projects, not a record. Docs:
-- Account > Projects and tickets.

CREATE TABLE projects (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The `project` tag value, such as `storefront`.
    name text NOT NULL CHECK (length(trim(name)) > 0),
    -- A regular expression for ticket keys, such as `^KD-\d+`.
    ticket_key_pattern text,
    -- Tags its token entries inherit, such as {"client": "acme"}.
    tags jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(tags) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    UNIQUE (organization_id, name),
    UNIQUE (organization_id, id)
);

-- One repository belongs to one project, such as `kestrel/storefront`.
CREATE TABLE project_repositories (
    organization_id uuid NOT NULL,
    repository text NOT NULL CHECK (repository ~ '^[^/\s]+(/[^/\s]+)+$'),
    project_id uuid NOT NULL,
    PRIMARY KEY (organization_id, repository),
    FOREIGN KEY (organization_id, project_id) REFERENCES projects (organization_id, id) ON DELETE CASCADE
);
CREATE INDEX project_repositories_project ON project_repositories (project_id);

-- Any unit of work. Only the key is required.
CREATE TABLE tickets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The `ticket` tag value, such as `KD-214` or `storefront#12`.
    key text NOT NULL CHECK (length(trim(key)) > 0),
    project_id uuid,
    title text,
    description text,
    parent_id uuid,
    assignee_id uuid,
    -- `manual`, or the ticket system it syncs from.
    source text NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'jira', 'azure_devops', 'github_issues')),
    -- Tags its token entries inherit, beyond the project's.
    tags jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(tags) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, key),
    UNIQUE (organization_id, id),
    CHECK (parent_id IS NULL OR parent_id <> id),
    FOREIGN KEY (organization_id, project_id) REFERENCES projects (organization_id, id),
    FOREIGN KEY (organization_id, parent_id) REFERENCES tickets (organization_id, id),
    FOREIGN KEY (organization_id, assignee_id) REFERENCES people (organization_id, id)
);
CREATE INDEX tickets_project ON tickets (project_id);
CREATE INDEX tickets_parent ON tickets (parent_id);
