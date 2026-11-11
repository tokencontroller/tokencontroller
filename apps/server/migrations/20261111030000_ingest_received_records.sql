-- Collection is holy (ADR 0028): every usage endpoint stores what it
-- received before it reads it. Received records live here, outside the
-- event log, with a time they are deleted after and a real delete.
-- Messages and activities read from them point back by id.
CREATE TABLE received_records (
    -- `rcv_` and a random part.
    id text PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The usage endpoint: `token-entries`, `token-entries-csv`,
    -- `otlp-logs`, `otlp-traces` or `otlp-metrics`.
    endpoint text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    -- The person the credential belongs to, by email.
    received_by text,
    -- How `body` is written, such as `application/json`.
    content_type text NOT NULL,
    -- What arrived, without content text unless `content_kept`.
    body bytea NOT NULL,
    -- The size as it arrived, before anything was removed: what the daily
    -- byte cap counts.
    bytes bigint NOT NULL CHECK (bytes >= 0),
    -- Whether the organization kept content text when it arrived.
    content_kept boolean NOT NULL DEFAULT false,
    delete_after timestamptz NOT NULL
);

CREATE INDEX received_records_organization_time ON received_records (organization_id, received_at);
CREATE INDEX received_records_delete_after ON received_records (delete_after);

-- Each time stored records are read again by a newer reader, back in
-- time. What a re-read makes does not count against the daily cap.
CREATE TABLE received_rereads (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The service version that read them, such as `1.0.0+abc1234`.
    reader_version text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    records bigint NOT NULL DEFAULT 0,
    messages bigint NOT NULL DEFAULT 0,
    activities bigint NOT NULL DEFAULT 0
);

-- Privacy by default: content text (prompts, responses, tool output, file
-- contents) is removed on arrival unless the organization switches this on.
ALTER TABLE organizations ADD COLUMN keep_content_text boolean NOT NULL DEFAULT false;
