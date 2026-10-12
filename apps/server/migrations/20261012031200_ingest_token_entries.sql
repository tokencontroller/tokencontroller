-- Token entries, the messages they are summed from, and their splits with
-- their tags (docs page Reference > Data model; ADR 0006, 0008, 0014).

CREATE TABLE token_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- Who answers for it. NULL only for telemetry from someone who is not a
    -- member yet; it shows as an error to people managers until taken.
    agent_manager_id UUID REFERENCES people (id),
    agent_harness TEXT NOT NULL,
    agent_name TEXT,
    model TEXT NOT NULL,
    -- Who charged for it, spelled as bills spell it: anthropic, openai,
    -- google, aws_bedrock, gcp_vertex, azure_foundry or other.
    provider TEXT NOT NULL,
    effort TEXT,
    speed TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NOT NULL CHECK (ended_at >= started_at),
    -- Sums of the counting messages.
    input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens BIGINT NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    cache_write_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0),
    cache_read_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_read_tokens >= 0),
    -- The agent's own figure, in nano-units of US dollars (ADR 0004).
    reported_cost_nanos BIGINT CHECK (reported_cost_nanos >= 0),
    -- The reported cost, or the token counts priced with the price table.
    -- NULL means unknown model: never shown as zero.
    list_value_nanos BIGINT,
    -- What the provider actually charged, where a bill or billed telemetry
    -- says so.
    billed_value_nanos BIGINT,
    currency TEXT NOT NULL DEFAULT 'USD',
    price_table_version_id BIGINT REFERENCES price_table_versions (id),
    summary TEXT,
    session_id TEXT,
    -- The sender's own stable ID, such as claude-code:<session id>.
    local_id TEXT,
    signals JSONB NOT NULL DEFAULT '{}',
    -- What the sender gave, kept so splits and tags can be built again when
    -- more messages of the session arrive: its tags (ticket and project
    -- included), its splits, and telemetry's resource attributes.
    sent_tags JSONB NOT NULL DEFAULT '{}',
    split_plan JSONB NOT NULL DEFAULT '[]',
    telemetry_attributes JSONB NOT NULL DEFAULT '{}',
    placed_by TEXT,
    status TEXT NOT NULL CHECK (status IN ('draft', 'submitted', 'approved', 'rejected', 'locked')),
    rejected_reason TEXT,
    -- Reviewed on a token sheet, or arrived automatically.
    on_token_sheet BOOLEAN NOT NULL,
    -- The door it came through last.
    door TEXT NOT NULL CHECK (door IN ('telemetry', 'endpoint', 'token_sheet', 'manual', 'csv')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One token entry per session (ADR 0018), and per sender ID.
CREATE UNIQUE INDEX token_entries_session ON token_entries (organization_id, session_id)
    WHERE session_id IS NOT NULL;
CREATE UNIQUE INDEX token_entries_local_id ON token_entries (organization_id, local_id)
    WHERE local_id IS NOT NULL;
CREATE INDEX token_entries_started ON token_entries (organization_id, started_at);
CREATE INDEX token_entries_agent_manager ON token_entries (agent_manager_id, started_at);
CREATE INDEX token_entries_created ON token_entries (created_at);

-- One model call. The same call seen through two doors is one row.
CREATE TABLE messages (
    organization_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    token_entry_id UUID REFERENCES token_entries (id) ON DELETE SET NULL,
    agent_harness TEXT NOT NULL,
    model TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens BIGINT NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    cache_write_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_write_tokens >= 0),
    cache_read_tokens BIGINT NOT NULL DEFAULT 0 CHECK (cache_read_tokens >= 0),
    reported_cost_nanos BIGINT CHECK (reported_cost_nanos >= 0),
    -- Reported cost, or tokens priced with the price table; NULL when the
    -- model is unknown.
    list_value_nanos BIGINT,
    request_id TEXT,
    -- real: the agent harness's own ID. built: made from the event, prefixed
    -- otel:. stand_in: one message for a token entry sent without messages.
    kind TEXT NOT NULL CHECK (kind IN ('real', 'built', 'stand_in')),
    from_telemetry BOOLEAN NOT NULL DEFAULT false,
    from_entry BOOLEAN NOT NULL DEFAULT false,
    -- Whether it is summed into its token entry (ADR 0018).
    counts BOOLEAN NOT NULL DEFAULT true,
    -- The split it belongs to, for splits by segment.
    split_position INTEGER,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, session_id, message_id)
);

CREATE INDEX messages_token_entry ON messages (token_entry_id);

-- A share of a token entry with its own tags. Most token entries have one.
CREATE TABLE splits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_entry_id UUID NOT NULL REFERENCES token_entries (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    -- by_message: exact, the split's messages. share: by hand, in percent.
    method TEXT NOT NULL CHECK (method IN ('whole', 'by_message', 'share')),
    share_percent INTEGER CHECK (share_percent > 0 AND share_percent <= 100),
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cache_write_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_tokens BIGINT NOT NULL DEFAULT 0,
    list_value_nanos BIGINT,
    billed_value_nanos BIGINT,
    signals JSONB NOT NULL DEFAULT '{}',
    -- What placed it, as the review list shows it: `rule: ticket key in
    -- branch`, `by hand`, `config file`, `matched by summary` or `no ticket`.
    placed_by TEXT NOT NULL,
    -- Its ticket matched a pattern but is not known yet; it fills in at the
    -- next ticket sync.
    ticket_not_synced BOOLEAN NOT NULL DEFAULT false,
    UNIQUE (token_entry_id, position)
);

-- Every tag of a split, inherited ones included, copied at arrival (ADR
-- 0008). One value per tag name.
CREATE TABLE split_tags (
    split_id UUID NOT NULL REFERENCES splits (id) ON DELETE CASCADE,
    organization_id UUID NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    -- Where it came from: sent (with the token entry or by hand), telemetry
    -- (resource attributes), rule, summary (matched by summary), ticket (the
    -- ticket and the project it brings), person (the agent manager) or
    -- inherited (from the person, project or ticket records).
    source TEXT NOT NULL,
    PRIMARY KEY (split_id, name)
);

CREATE INDEX split_tags_filter ON split_tags (organization_id, name, value);
