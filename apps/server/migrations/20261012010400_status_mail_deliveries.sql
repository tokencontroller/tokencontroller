-- The sign-in mail signal /api/v1/status reports and
-- status.tokencontroller.com reads. Job heartbeats live in the jobs area's
-- own table.

-- Every mail the service tried to send: kind and outcome only, never the
-- address or the content.
CREATE TABLE mail_deliveries (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind text NOT NULL,
    sent boolean NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX mail_deliveries_recent ON mail_deliveries (created_at);
