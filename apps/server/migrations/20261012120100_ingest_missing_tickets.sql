-- Ticket keys that matched a project's pattern but that a ticket sync,
-- run after the token entry arrived, still did not find (docs: Attribute >
-- Ticket key patterns, unknown). Rules and the summary match run again
-- without them. A key leaves the list once a sync finds its ticket.
CREATE TABLE missing_tickets (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    key text NOT NULL,
    missed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, key)
);

-- The missing ticket key a split was placed without, or still named by
-- hand. The review list shows it as "unknown ticket".
ALTER TABLE splits ADD COLUMN unknown_ticket text;
