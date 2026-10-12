-- Customer relations, part two: feedback reports from the web app's
-- feedback button, notes on a customer for the customer record, and support
-- messages from inside the app.

-- A report from the feedback button. The person sees what it contains before
-- sending it (docs: Security and privacy > What a feedback report contains).
CREATE TABLE feedback_reports (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL,
    person_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('bug', 'idea', 'question')),
    body text NOT NULL,
    -- Off unless the person turned it on, and saw it, before sending.
    screenshot bytea,
    screenshot_type text CHECK (screenshot_type IN ('image/png', 'image/jpeg')),
    -- Page, app version, browser, screen size, recent error messages and
    -- request IDs. Never request contents.
    diagnostics jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(diagnostics) = 'object'),
    -- received: waiting for us; answered: we answered; fixed: the fix shipped.
    status text NOT NULL DEFAULT 'received' CHECK (status IN ('received', 'answered', 'fixed')),
    answer text,
    -- The version the fix shipped in, such as 1.0.1.
    fixed_in text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    -- When the people who handle reports were told about it.
    reviewers_notified_at timestamptz,
    -- When the reporter was last told about an answer or a fix; empty while
    -- a notice is due.
    reporter_notified_at timestamptz,
    CHECK ((screenshot IS NULL) = (screenshot_type IS NULL)),
    FOREIGN KEY (organization_id, person_id) REFERENCES people (organization_id, id) ON DELETE CASCADE
);
CREATE INDEX feedback_reports_status ON feedback_reports (status, created_at);
CREATE INDEX feedback_reports_person ON feedback_reports (person_id, created_at);
CREATE INDEX feedback_reports_organization ON feedback_reports (organization_id);

-- What the operators learned about a customer: call notes, answers to
-- "What was hard this month?", anything worth keeping with the record.
CREATE TABLE customer_notes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The operator who wrote it, kept as an address because operators may
    -- belong to another organization.
    author_email text NOT NULL,
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX customer_notes_organization ON customer_notes (organization_id, created_at);

-- Support messages sent from inside the app, by a signed-in member.
ALTER TABLE support_conversations DROP CONSTRAINT support_conversations_channel_check;
ALTER TABLE support_conversations ADD CONSTRAINT support_conversations_channel_check
    CHECK (channel IN ('mail', 'contact_form', 'app'));
