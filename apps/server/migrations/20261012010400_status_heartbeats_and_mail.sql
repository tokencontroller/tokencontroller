-- The signals /api/v1/status reports and status.tokencontroller.com reads.

-- The last run of each background job. The job runner records every run;
-- a job whose last success is too old shows as degraded.
CREATE TABLE job_heartbeats (
    job text PRIMARY KEY CHECK (job ~ '^[a-z][a-z0-9-]*$'),
    last_success_at timestamptz,
    last_failure_at timestamptz,
    -- Why the last failed run failed, in plain words, without secrets.
    last_error text,
    -- How often the job should succeed, in seconds, so a missed run shows.
    expected_every_seconds integer NOT NULL CHECK (expected_every_seconds > 0)
);

-- Every mail the service tried to send: kind and outcome only, never the
-- address or the content.
CREATE TABLE mail_deliveries (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind text NOT NULL,
    sent boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX mail_deliveries_recent ON mail_deliveries (created_at);
