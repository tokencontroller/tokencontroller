-- Price tables (docs page Reference > Price tables): the published price per
-- model, refreshed daily from LiteLLM and models.dev. Every change makes a
-- new version; a closed period keeps the version it closed with (ADR 0007),
-- so versions are never changed or deleted.
CREATE TABLE price_table_versions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- When this version took effect.
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A hash of the rows, to tell whether a refresh changed anything.
    digest TEXT NOT NULL,
    -- Sources that could not be read in the refresh that made this version.
    failed_sources TEXT[] NOT NULL DEFAULT '{}',
    -- The rows: model, provider, and USD per million tokens by kind, in
    -- nano-units (ADR 0004).
    prices JSONB NOT NULL
);

CREATE INDEX price_table_versions_created_at ON price_table_versions (created_at);
