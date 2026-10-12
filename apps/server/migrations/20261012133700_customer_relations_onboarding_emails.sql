-- Onboarding emails (customer relations, loop 5): at most one per
-- organization and first step, ever. The row is written before the email
-- goes out, so two runs never send the same step twice.

CREATE TABLE onboarding_emails (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    step text NOT NULL CHECK (step IN ('token_entry', 'token_sheet', 'bill', 'close')),
    sent_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, step)
);
