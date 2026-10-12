-- Token budgets and token forecasts (docs: Account > Token budgets and
-- forecasts).

-- Money a person planned for a filter. `period` set and `every_month` off:
-- that one month. `every_month` on: every month from `period` on. No period:
-- all time, as for a ticket.
CREATE TABLE token_budgets (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text,
    filter jsonb NOT NULL CHECK (jsonb_typeof(filter) = 'object'),
    period date CHECK (period IS NULL OR extract(day FROM period) = 1),
    every_month boolean NOT NULL DEFAULT false,
    CHECK (NOT every_month OR period IS NOT NULL),
    amount_nanos bigint NOT NULL CHECK (amount_nanos > 0),
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    token_forecast_id uuid,
    set_by uuid NOT NULL REFERENCES people (id),
    set_at timestamptz NOT NULL DEFAULT now(),
    removed_at timestamptz
);

CREATE INDEX token_budgets_by_organization ON token_budgets (organization_id) WHERE removed_at IS NULL;

-- The embedding of a token entry's summary, in millionths, made with
-- `model`. Only summaries are embedded; no other text exists.
CREATE TABLE summary_embeddings (
    token_entry_id uuid PRIMARY KEY REFERENCES token_entries (id) ON DELETE CASCADE,
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    model text NOT NULL,
    millionths integer[] NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX summary_embeddings_by_organization ON summary_embeddings (organization_id, model);

-- A suggestion for one ticket: the matched past tickets with their
-- confidence and cost, and the median proposed. It becomes a token budget
-- only when a person accepts it.
CREATE TABLE token_forecasts (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    ticket_id uuid NOT NULL REFERENCES tickets (id) ON DELETE CASCADE,
    proposed_nanos bigint,
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    matches jsonb NOT NULL CHECK (jsonb_typeof(matches) = 'array'),
    created_by uuid NOT NULL REFERENCES people (id),
    created_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE token_budgets
    ADD FOREIGN KEY (token_forecast_id) REFERENCES token_forecasts (id) ON DELETE SET NULL;

-- Token forecasts can be switched off per organization.
CREATE TABLE forecast_settings (
    organization_id uuid PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    enabled boolean NOT NULL
);
