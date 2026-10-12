-- Bills and their revisions (docs: Account > Bills, Reference > Bill schema),
-- and the exchange rates bills and reports are converted with.
--
-- A bill is one provider's figure for one period and one cost source. A
-- provider can restate it, so every change is a new revision; nothing is
-- overwritten. Amounts are nano-units of the bill's own currency (ADR 0004).

CREATE TABLE bills (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    -- The period: the first day of the calendar month.
    period date NOT NULL CHECK (extract(day FROM period) = 1),
    provider text NOT NULL CHECK (provider ~ '^[a-z0-9_]{1,40}$'),
    -- Only metered cost sources have bills: a seat or a subscription is a
    -- flat fee and has none.
    cost_source text NOT NULL CHECK (
        cost_source IN ('usage_at_api_rates', 'api_key', 'cloud_marketplace', 'reseller')
    ),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, period, provider, cost_source)
);

CREATE TABLE bill_revisions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    bill_id uuid NOT NULL REFERENCES bills (id) ON DELETE CASCADE,
    revision integer NOT NULL CHECK (revision > 0),
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    gross_nanos bigint NOT NULL,
    discount_nanos bigint NOT NULL DEFAULT 0 CHECK (discount_nanos >= 0),
    credit_nanos bigint NOT NULL DEFAULT 0 CHECK (credit_nanos >= 0),
    net_nanos bigint NOT NULL,
    CHECK (gross_nanos - discount_nanos - credit_nanos = net_nanos),
    -- To the organization's reporting currency, at the bill's date (the
    -- period's last day). Given with the bill, or filled from
    -- exchange_rates; NULL until a rate for that day is known.
    exchange_rate_nanos bigint CHECK (exchange_rate_nanos > 0),
    exchange_rate_given boolean NOT NULL DEFAULT false,
    source text NOT NULL CHECK (source IN ('pulled', 'csv', 'manual', 'api')),
    -- The bill's own detail as the provider sent it, such as cost per
    -- workspace and day. Shown, never summed.
    lines jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(lines) = 'array'),
    recorded_by uuid REFERENCES people (id),
    recorded_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (bill_id, revision)
);

CREATE INDEX bill_revisions_latest ON bill_revisions (bill_id, revision DESC);

-- Reference rates per day, as units of the currency per one euro (the
-- European Central Bank publishes them this way). EUR itself is not stored.
CREATE TABLE exchange_rates (
    day date NOT NULL,
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$' AND currency <> 'EUR'),
    per_euro_nanos bigint NOT NULL CHECK (per_euro_nanos > 0),
    PRIMARY KEY (currency, day)
);
