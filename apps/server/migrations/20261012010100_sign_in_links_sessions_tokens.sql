-- Magic links, browser sessions and API tokens. Only SHA-256 hashes of the
-- secrets are stored. Docs: Integrations > Sign-in providers, Reference >
-- HTTP API.

-- A sign-in link sent by email. It works once and for 15 minutes.
CREATE TABLE sign_in_links (
    token_hash bytea PRIMARY KEY,
    email text NOT NULL,
    -- For a sign-up, the organization to create once the link is used.
    sign_up_organization_name text,
    -- For a local tool such as `tc connect`: the hash of the secret the tool
    -- holds to collect its session once the person used the link.
    poll_hash bytea UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    -- Set when the person used the link, with who they signed in as.
    used_at timestamptz,
    person_id uuid REFERENCES people (id) ON DELETE CASCADE,
    -- Set when the local tool collected its session.
    polled_at timestamptz
);
CREATE INDEX sign_in_links_email ON sign_in_links (email, created_at);

-- A signed-in browser or local tool.
CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY,
    person_id uuid NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    -- `browser` (a cookie) or `local_tool` (a `tc_ses_` bearer token).
    client text NOT NULL CHECK (client IN ('browser', 'local_tool')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_person ON sessions (person_id);

-- API tokens: `tc_svc_` service tokens (the creator is the agent manager of
-- what they report), `tc_read_` read tokens (see what the creator's role
-- sees) and `tc_org_` organization tokens (telemetry push).
CREATE TABLE api_tokens (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('service', 'read', 'organization')),
    -- A label, such as "review bot".
    name text NOT NULL DEFAULT '',
    -- The person who created it, or took it over. For a service token this is
    -- the agent manager of every token entry it reports.
    person_id uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE,
    -- The first characters, such as `tc_svc_3f9a`, to tell tokens apart.
    shown_prefix text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at timestamptz,
    FOREIGN KEY (organization_id, person_id) REFERENCES people (organization_id, id) ON DELETE CASCADE
);
CREATE INDEX api_tokens_organization ON api_tokens (organization_id);
