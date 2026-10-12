-- Organizations, people (members), their roles and tags, invites, and the
-- filters people managers are assigned. Docs: Account > Organization
-- settings, Roles; Reference > Data model.

-- One customer of Token Controller. One account belongs to one organization.
CREATE TABLE organizations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL CHECK (length(trim(name)) > 0),
    -- Short name used in invite links, such as `kestrel` in /i/kestrel-4f2a.
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    -- How the provider charges; NULL until the controller picks one ("none
    -- yet" in the quick start).
    cost_source text CHECK (cost_source IN (
        'subscription', 'seat', 'usage_at_api_rates', 'api_key',
        'cloud_marketplace', 'reseller'
    )),
    -- ISO 4217 code of the one currency the organization computes in.
    reporting_currency text NOT NULL DEFAULT 'USD' CHECK (reporting_currency ~ '^[A-Z]{3}$'),
    approval boolean NOT NULL DEFAULT false,
    -- Email domains magic-link sign-in is limited to; empty means any.
    sign_in_domains text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);

-- A member of an organization. The default cost center.
CREATE TABLE people (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    email text NOT NULL CHECK (email = lower(trim(email)) AND email LIKE '%_@_%'),
    name text NOT NULL DEFAULT '',
    -- Roles stack: each includes everything the one before it can do.
    role text NOT NULL DEFAULT 'agent_manager'
        CHECK (role IN ('agent_manager', 'people_manager', 'controller')),
    -- Tags its new token entries inherit, such as {"team": "web"}. One value
    -- per tag name, so a JSON object of strings.
    tags jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(tags) = 'object'),
    -- An invited person who has not signed in yet has no joined_at.
    invited_at timestamptz NOT NULL DEFAULT now(),
    joined_at timestamptz,
    -- People who leave are kept, so past token entries keep their agent
    -- manager. Removed people cannot sign in and do not count as members.
    removed_at timestamptz,
    UNIQUE (organization_id, id)
);

-- One account, one organization: an email is a member of at most one
-- organization at a time.
CREATE UNIQUE INDEX people_email_active ON people (email) WHERE removed_at IS NULL;
CREATE INDEX people_organization ON people (organization_id);

-- The code in a person's invite link, such as `kestrel-4f2a`. Opening it
-- leads to sign-in with the email filled in; `tc connect` takes the same link.
CREATE TABLE invites (
    code text PRIMARY KEY CHECK (code ~ '^[a-z0-9-]+$'),
    organization_id uuid NOT NULL,
    person_id uuid NOT NULL,
    created_by uuid REFERENCES people (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, person_id) REFERENCES people (organization_id, id) ON DELETE CASCADE
);
CREATE INDEX invites_person ON invites (person_id);

-- The filters a people manager reads and approves, such as {"team": "web"}
-- or {"team": "web", "project": "storefront"}. A token entry matches a
-- filter when it has every tag in it.
CREATE TABLE people_manager_filters (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL,
    person_id uuid NOT NULL,
    filter jsonb NOT NULL CHECK (jsonb_typeof(filter) = 'object' AND filter <> '{}'),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, person_id) REFERENCES people (organization_id, id) ON DELETE CASCADE,
    UNIQUE (person_id, filter)
);
