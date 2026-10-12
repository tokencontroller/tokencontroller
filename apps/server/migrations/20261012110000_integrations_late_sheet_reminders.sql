-- Late token sheet reminders sent: one mail per late token sheet, so the
-- reminder job never sends a second one. Docs: Account > Cadence and token
-- sheets.
CREATE TABLE late_sheet_reminders (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    person_id uuid NOT NULL REFERENCES people (id) ON DELETE CASCADE,
    -- The first day of the late token sheet's cadence period.
    starts_on date NOT NULL,
    sent_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, person_id, starts_on)
);
