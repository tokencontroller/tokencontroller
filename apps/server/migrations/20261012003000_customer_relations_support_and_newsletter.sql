-- Customer relations, part one: the support inbox, the site assistant and the
-- newsletter. All of it is off unless its settings are set, so a self-hosted
-- instance keeps these tables empty.

-- Mail as the mail worker handed it over, kept so a repeat hand-over with the
-- same id is harmless and a message can be read again as it arrived.
CREATE TABLE inbound_mail (
    message_id text PRIMARY KEY,
    mailbox text NOT NULL,
    envelope_from text NOT NULL,
    envelope_to text NOT NULL,
    raw bytea NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now()
);

-- One support conversation: a mail and its replies, a contact form message,
-- or anything else someone asked us.
CREATE TABLE support_conversations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    channel text NOT NULL CHECK (channel IN ('mail', 'contact_form')),
    contact_email text NOT NULL,
    contact_name text,
    subject text NOT NULL,
    -- What the conversation is about, as the support answerer judged it.
    kind text NOT NULL DEFAULT 'question'
        CHECK (kind IN ('question', 'feature_request', 'bug', 'privacy_request', 'sales', 'automatic', 'spam', 'other')),
    -- new: waiting for the support answerer; answered: waiting for the person;
    -- escalated: waiting for the operator; closed: nothing left to do.
    state text NOT NULL DEFAULT 'new' CHECK (state IN ('new', 'answered', 'escalated', 'closed')),
    -- Why it went to the operator, and the answer the support answerer proposes.
    escalation_reason text,
    suggested_answer text,
    -- The support answerer's tries, and the earliest time for the next one.
    attempts integer NOT NULL DEFAULT 0,
    answer_after timestamptz NOT NULL DEFAULT now(),
    -- When the operators were told about the escalation.
    notified_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX support_conversations_state ON support_conversations (state, answer_after);
CREATE INDEX support_conversations_contact ON support_conversations (lower(contact_email));

CREATE TABLE support_messages (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id uuid NOT NULL REFERENCES support_conversations (id) ON DELETE CASCADE,
    -- customer: the person who wrote to us; assistant: the support answerer;
    -- operator: a person who runs the service.
    author text NOT NULL CHECK (author IN ('customer', 'assistant', 'operator')),
    body text NOT NULL,
    -- Docs pages the answer links to.
    links text[] NOT NULL DEFAULT '{}',
    -- The mail's Message-ID, so replies find their conversation.
    mail_message_id text,
    inbound_message_id text REFERENCES inbound_mail (message_id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- Answers go out by mail; until then sent_at is empty.
    sent_at timestamptz,
    send_attempts integer NOT NULL DEFAULT 0
);
CREATE INDEX support_messages_conversation ON support_messages (conversation_id, created_at);
CREATE UNIQUE INDEX support_messages_mail_message_id ON support_messages (mail_message_id) WHERE mail_message_id IS NOT NULL;
CREATE INDEX support_messages_unsent ON support_messages (created_at) WHERE sent_at IS NULL AND author <> 'customer';

-- Questions asked to the site assistant on the public website, and what it
-- answered. No IP address or other visitor data.
CREATE TABLE assistant_questions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    question text NOT NULL,
    -- The website page it was asked on.
    page text,
    answer text NOT NULL,
    -- Whether the docs answered it. A missed question becomes a docs change.
    answered boolean NOT NULL,
    links text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX assistant_questions_created ON assistant_questions (created_at);

-- The newsletter's own list, with double opt-in. Unsubscribing deletes the
-- row, and a sign-up not confirmed within 7 days is deleted too.
CREATE TABLE newsletter_subscribers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email text NOT NULL,
    -- Where the sign-up came from, such as a page of the website.
    source text NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'confirmed')),
    confirm_token text NOT NULL UNIQUE,
    unsubscribe_token text NOT NULL UNIQUE,
    requested_at timestamptz NOT NULL DEFAULT now(),
    -- The confirmation mail went out; empty until it did.
    confirmation_sent_at timestamptz,
    confirmed_at timestamptz
);
CREATE UNIQUE INDEX newsletter_subscribers_email ON newsletter_subscribers (lower(email));

CREATE TABLE newsletter_issues (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    subject text NOT NULL,
    body text NOT NULL,
    -- draft: written; sending: approved and going out; sent: done.
    status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'sending', 'sent')),
    created_at timestamptz NOT NULL DEFAULT now(),
    send_started_at timestamptz,
    sent_at timestamptz
);

CREATE TABLE newsletter_deliveries (
    issue_id uuid NOT NULL REFERENCES newsletter_issues (id) ON DELETE CASCADE,
    subscriber_id uuid NOT NULL REFERENCES newsletter_subscribers (id) ON DELETE CASCADE,
    sent_at timestamptz,
    PRIMARY KEY (issue_id, subscriber_id)
);
