-- A controller asking everyone to submit their token sheets before the
-- monthly close (docs: Account > Cadence and token sheets, late). Asking
-- again moves the time; the request shows until the period closes.
CREATE TABLE submit_requests (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The first day of the month to submit for.
    period date NOT NULL CHECK (extract(day FROM period) = 1),
    requested_by uuid NOT NULL REFERENCES people (id),
    requested_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, period)
);
