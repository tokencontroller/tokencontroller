-- One row per background job (ADR 0002): when it last ran, whether it
-- worked, and whether someone asked for a run now. The runner takes a
-- Postgres advisory lock per job, so with several replicas each run happens
-- once; this table is what replicas agree on and what the status endpoint
-- reads as the job's heartbeat.
CREATE TABLE job_heartbeats (
    job TEXT PRIMARY KEY,
    last_started_at TIMESTAMPTZ,
    last_finished_at TIMESTAMPTZ,
    last_succeeded_at TIMESTAMPTZ,
    last_error TEXT,
    runs BIGINT NOT NULL DEFAULT 0,
    failures_in_a_row INTEGER NOT NULL DEFAULT 0,
    requested_at TIMESTAMPTZ
);
