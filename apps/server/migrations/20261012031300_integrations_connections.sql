-- Bill connections and ticket systems: what an organization connected, with
-- its key sealed with TC_SECRET_KEY, and how its last pull or sync went.
-- Docs: Integrations.

-- A bill source the service pulls from: an Anthropic Enterprise Analytics
-- key, an Anthropic Console admin key or an OpenAI admin key.
CREATE TABLE bill_connections (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    provider text NOT NULL
        CHECK (provider IN ('anthropic_enterprise', 'anthropic_console', 'openai')),
    -- The key, sealed; never sent back.
    sealed_key text NOT NULL,
    -- The key's last characters, so people can tell keys apart.
    key_hint text NOT NULL,
    created_by uuid REFERENCES people (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- Someone asked for a pull now.
    pull_requested_at timestamptz,
    last_pull_started_at timestamptz,
    -- When the last pull succeeded.
    last_pull_at timestamptz,
    -- Why the last pull failed; NULL once one succeeds.
    last_error text
);
CREATE INDEX bill_connections_organization ON bill_connections (organization_id);

-- A ticket system tickets sync from.
CREATE TABLE ticket_systems (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('jira', 'azure_devops', 'github_issues')),
    -- The Jira Cloud site, the Azure DevOps organization or the GitHub API.
    site text NOT NULL,
    -- The API token, and for Jira Cloud the account's email, sealed.
    sealed_credentials text NOT NULL,
    key_hint text NOT NULL,
    created_by uuid REFERENCES people (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    sync_requested_at timestamptz,
    last_sync_started_at timestamptz,
    last_sync_at timestamptz,
    last_error text,
    UNIQUE (organization_id, id)
);

-- Which project of the ticket system feeds which Token Controller project.
CREATE TABLE ticket_system_projects (
    organization_id uuid NOT NULL,
    ticket_system_id uuid NOT NULL,
    -- The Jira project key, the Azure DevOps project ID or the GitHub
    -- repository as owner/name.
    external_project text NOT NULL,
    external_name text NOT NULL,
    project_id uuid NOT NULL,
    -- Tickets changed before this are synced: the start of the last
    -- successful sync of this project, less a few minutes.
    synced_until timestamptz,
    PRIMARY KEY (ticket_system_id, external_project),
    FOREIGN KEY (organization_id, ticket_system_id)
        REFERENCES ticket_systems (organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, project_id)
        REFERENCES projects (organization_id, id) ON DELETE CASCADE
);
CREATE INDEX ticket_system_projects_project ON ticket_system_projects (project_id);
