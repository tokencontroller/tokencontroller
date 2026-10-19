-- Cost model 2 (ADR 0022): the organization is an append-only event log, and
-- every figure is a fold of it as of a date. The tables of the earlier
-- model go; the product has no customers yet, so nothing is carried over.

-- The earlier model's tables.
DROP TRIGGER IF EXISTS close_lock_token_entries ON token_entries;
DROP TRIGGER IF EXISTS close_lock_splits ON splits;
DROP TRIGGER IF EXISTS close_lock_split_tags ON split_tags;
DROP FUNCTION IF EXISTS close_refuse_locked_entry();
DROP FUNCTION IF EXISTS close_refuse_locked_split();
DROP FUNCTION IF EXISTS close_refuse_locked_split_tag();
DROP TABLE IF EXISTS
    adjustments, cost_sheet_links, closed_period_bills, closed_period_rates, closed_periods,
    cadences, token_sheet_entries, token_entry_decisions, token_sheets,
    token_budgets, summary_embeddings, token_forecasts, forecast_settings,
    late_sheet_reminders, submit_requests, missing_tickets, saved_reports,
    split_tags, splits, messages, token_entries,
    bill_revisions, bills, price_table_versions,
    tickets, project_repositories, projects, tag_map_versions,
    people_manager_filters, ticket_system_projects
    CASCADE;

-- The event log: one row per event, per organization. The payload is the
-- core crate's `Event` record as JSON. Nothing here is ever updated or
-- deleted; a correction is a new event.
CREATE TABLE events (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    id text NOT NULL CHECK (length(id) BETWEEN 1 AND 200),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    -- The person who recorded it, as the tag value `person`: their email.
    recorded_by text,
    supersedes text,
    -- The record's kind in the words of the docs: message, bill, tag value, …
    kind text NOT NULL,
    payload jsonb NOT NULL,
    PRIMARY KEY (organization_id, id)
);
CREATE INDEX events_in_order ON events (organization_id, recorded_at, id);
CREATE INDEX events_by_kind ON events (organization_id, kind, recorded_at);
CREATE INDEX events_supersedes ON events (organization_id, supersedes) WHERE supersedes IS NOT NULL;

-- Which event holds each message, so a message seen again is the same
-- record, and which entry ids were already taken, so sending a token entry
-- again adds nothing.
CREATE TABLE message_events (
    organization_id uuid NOT NULL,
    session_id text NOT NULL,
    message_id text NOT NULL,
    event_id text NOT NULL,
    PRIMARY KEY (organization_id, session_id, message_id),
    FOREIGN KEY (organization_id, event_id) REFERENCES events (organization_id, id) ON DELETE CASCADE
);
CREATE TABLE entry_ids (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    local_id text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, local_id)
);

-- The managed price table (ADR 0024): the rows of the file TC_PRICE_TABLE
-- names, or the rows an Enterprise key brings. One table for the instance;
-- every organization's fold reads them as managed price table rows.
CREATE TABLE managed_price_rows (
    provider text NOT NULL,
    model text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('input', 'output', 'cache_write', 'cache_read')),
    per_million_nanos bigint NOT NULL CHECK (per_million_nanos >= 0),
    valid_from date NOT NULL,
    loaded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, model, kind, valid_from)
);

-- Organizations: the cutoff, approval and the reporting currency are what
-- is left. The cutoff itself is an event; the rules moved into the log.
ALTER TABLE organizations
    DROP COLUMN IF EXISTS cost_source,
    DROP COLUMN IF EXISTS rules,
    DROP COLUMN IF EXISTS several_keys,
    DROP COLUMN IF EXISTS summary_match;

-- People: the three roles are agent manager, people manager and org
-- manager (ADR 0023). Attributes and rules are events.
ALTER TABLE people
    DROP COLUMN IF EXISTS tags,
    DROP COLUMN IF EXISTS rules,
    DROP CONSTRAINT IF EXISTS people_role_check;
UPDATE people SET role = 'org_manager' WHERE role = 'controller';
ALTER TABLE people
    ADD CONSTRAINT people_role_check
        CHECK (role IN ('agent_manager', 'people_manager', 'org_manager'));

ALTER TABLE sign_in_links DROP COLUMN IF EXISTS sign_up_cost_source;

-- Tag names and attribute names share one list, told apart by kind. The
-- built-in tag names are project, feature and client; the built-in
-- attribute names team, department and location.
ALTER TABLE tag_names ADD COLUMN kind text NOT NULL DEFAULT 'tag'
    CHECK (kind IN ('tag', 'attribute'));
DELETE FROM tag_names WHERE name IN ('person', 'ticket');
INSERT INTO tag_names (organization_id, name, built_in, kind)
    SELECT o.id, n.name, true, n.kind
    FROM organizations o,
         (VALUES ('project', 'tag'), ('feature', 'tag'), ('client', 'tag'),
                 ('team', 'attribute'), ('department', 'attribute'), ('location', 'attribute')) AS n(name, kind)
    ON CONFLICT (organization_id, name) DO UPDATE SET built_in = true, kind = EXCLUDED.kind;

-- Ticket systems: Jira Cloud only. The sync map names the project tag
-- value each Jira project feeds, and the fields that are copied.
DELETE FROM ticket_systems WHERE kind <> 'jira';
ALTER TABLE ticket_systems DROP CONSTRAINT IF EXISTS ticket_systems_kind_check;
ALTER TABLE ticket_systems ADD CONSTRAINT ticket_systems_kind_check CHECK (kind = 'jira');
CREATE TABLE ticket_system_projects (
    organization_id uuid NOT NULL,
    ticket_system_id uuid NOT NULL,
    external_project text NOT NULL,
    external_name text NOT NULL,
    -- The key of the project tag value the issues get as their default.
    project text NOT NULL,
    -- The Jira fields copied onto each feature: title, description,
    -- type, status, parent, assignee.
    fields text[] NOT NULL DEFAULT '{title,description,type,status,parent,assignee}',
    synced_until timestamptz,
    PRIMARY KEY (ticket_system_id, external_project),
    FOREIGN KEY (organization_id, ticket_system_id)
        REFERENCES ticket_systems (organization_id, id) ON DELETE CASCADE
);

-- Saved reports: a filter, a grouping and a range, kept for the
-- organization until reversed.
CREATE TABLE saved_reports (
    id text PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    filter jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(filter) = 'object'),
    grouped_by text,
    from_day date,
    to_day date,
    created_by uuid NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    reversed_at timestamptz
);
CREATE INDEX saved_reports_by_organization ON saved_reports (organization_id);

-- An org manager asks everyone to submit before moving the cutoff; each
-- person gets one mail per request.
CREATE TABLE submit_requests (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    requested_by uuid NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    requested_at timestamptz NOT NULL DEFAULT now(),
    -- The day the request asks people to have submitted up to.
    until_day date NOT NULL,
    message text NOT NULL DEFAULT ''
);
CREATE INDEX submit_requests_by_organization ON submit_requests (organization_id, requested_at);
CREATE TABLE submit_reminders (
    request_id uuid NOT NULL REFERENCES submit_requests (id) ON DELETE CASCADE,
    person_id uuid NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    sent_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (request_id, person_id)
);

-- What a bill event carries beside the core's figure: its rate to the
-- reporting currency, where it came from, whether it is final and the
-- provider's own lines.
CREATE TABLE bill_details (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    event_id text NOT NULL,
    exchange_rate_nanos bigint,
    exchange_rate_given boolean NOT NULL DEFAULT false,
    source text NOT NULL,
    is_final boolean NOT NULL DEFAULT true,
    lines jsonb NOT NULL DEFAULT '[]'::jsonb,
    PRIMARY KEY (organization_id, event_id),
    FOREIGN KEY (organization_id, event_id) REFERENCES events (organization_id, id) ON DELETE CASCADE
);
