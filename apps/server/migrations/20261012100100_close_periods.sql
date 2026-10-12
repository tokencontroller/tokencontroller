-- The monthly close (ADR 0007, docs: Account > Monthly close, Adjustments).
--
-- A period is open until a row exists here. Closing records the tag map and
-- price table versions the cost sheet is computed with, and each bill
-- revision it was reconciled against with the rate it was converted at.
-- Token entries, splits and tags of a closed period are locked by the
-- triggers below; the only change after the close is an adjustment.

CREATE TABLE closed_periods (
    organization_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    period date NOT NULL CHECK (extract(day FROM period) = 1),
    closed_at timestamptz NOT NULL DEFAULT now(),
    closed_by uuid NOT NULL REFERENCES people (id),
    reporting_currency text NOT NULL CHECK (reporting_currency ~ '^[A-Z]{3}$'),
    approval boolean NOT NULL,
    -- NULL: the organization never changed its tag map, so the default
    -- ladder (ticket, project, organization) applied.
    tag_map_version_id uuid REFERENCES tag_map_versions (id),
    price_table_version_id uuid,
    PRIMARY KEY (organization_id, period)
);

-- Each bill as it was reconciled at the close, in the bill's currency.
CREATE TABLE closed_period_bills (
    organization_id uuid NOT NULL,
    period date NOT NULL,
    bill_revision_id uuid NOT NULL REFERENCES bill_revisions (id),
    entries_nanos bigint NOT NULL,
    residual_nanos bigint NOT NULL,
    exchange_rate_nanos bigint NOT NULL CHECK (exchange_rate_nanos > 0),
    PRIMARY KEY (organization_id, period, bill_revision_id),
    FOREIGN KEY (organization_id, period)
        REFERENCES closed_periods (organization_id, period) ON DELETE CASCADE
);

-- The rate each other currency was converted to the reporting currency at:
-- that of the period's last day.
CREATE TABLE closed_period_rates (
    organization_id uuid NOT NULL,
    period date NOT NULL,
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    exchange_rate_nanos bigint NOT NULL CHECK (exchange_rate_nanos > 0),
    PRIMARY KEY (organization_id, period, currency),
    FOREIGN KEY (organization_id, period)
        REFERENCES closed_periods (organization_id, period) ON DELETE CASCADE
);

-- A signed correction to a closed period, in the reporting currency.
-- `tags` say where it sits, such as {"ticket": "KD-214"}. A late token
-- sheet posts one adjustment per split, marked late, linked to the split.
CREATE TABLE adjustments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL,
    period date NOT NULL,
    amount_nanos bigint NOT NULL CHECK (amount_nanos <> 0),
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    tags jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(tags) = 'object'),
    reason text NOT NULL CHECK (length(btrim(reason)) > 0),
    late boolean NOT NULL DEFAULT false,
    token_entry_id uuid REFERENCES token_entries (id) ON DELETE SET NULL,
    split_id uuid REFERENCES splits (id) ON DELETE SET NULL,
    posted_by uuid REFERENCES people (id),
    posted_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, period)
        REFERENCES closed_periods (organization_id, period) ON DELETE CASCADE,
    CHECK (late OR posted_by IS NOT NULL)
);

CREATE INDEX adjustments_by_period ON adjustments (organization_id, period);
CREATE UNIQUE INDEX adjustments_one_per_late_split ON adjustments (split_id) WHERE late;

-- A link that shows one closed period's cost sheet without signing in, for
-- finance. Only a hash of the link's secret is kept.
CREATE TABLE cost_sheet_links (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL,
    period date NOT NULL,
    secret_hash text NOT NULL UNIQUE,
    created_by uuid NOT NULL REFERENCES people (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    FOREIGN KEY (organization_id, period)
        REFERENCES closed_periods (organization_id, period) ON DELETE CASCADE
);

-- Locks (ADR 0007). A locked token entry, its splits and their tags cannot
-- change or go away, except when their whole organization is deleted.

CREATE FUNCTION close_refuse_locked_entry() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status = 'locked'
        AND EXISTS (SELECT 1 FROM organizations WHERE id = OLD.organization_id) THEN
        RAISE EXCEPTION 'token entry % is in a closed period and cannot change', OLD.id
            USING ERRCODE = 'check_violation', HINT = 'post an adjustment instead';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER close_lock_token_entries
    BEFORE UPDATE OR DELETE ON token_entries
    FOR EACH ROW EXECUTE FUNCTION close_refuse_locked_entry();

CREATE FUNCTION close_refuse_locked_split() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    entry_id uuid;
BEGIN
    IF TG_OP = 'INSERT' THEN
        entry_id := NEW.token_entry_id;
    ELSE
        entry_id := OLD.token_entry_id;
    END IF;
    IF EXISTS (SELECT 1 FROM token_entries WHERE id = entry_id AND status = 'locked') THEN
        RAISE EXCEPTION 'token entry % is in a closed period and its splits cannot change', entry_id
            USING ERRCODE = 'check_violation', HINT = 'post an adjustment instead';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER close_lock_splits
    BEFORE INSERT OR UPDATE OR DELETE ON splits
    FOR EACH ROW EXECUTE FUNCTION close_refuse_locked_split();

CREATE FUNCTION close_refuse_locked_split_tag() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    tagged_split uuid;
BEGIN
    IF TG_OP = 'INSERT' THEN
        tagged_split := NEW.split_id;
    ELSE
        tagged_split := OLD.split_id;
    END IF;
    IF EXISTS (
        SELECT 1 FROM splits s JOIN token_entries e ON e.id = s.token_entry_id
        WHERE s.id = tagged_split AND e.status = 'locked'
    ) THEN
        RAISE EXCEPTION 'split % is in a closed period and its tags cannot change', tagged_split
            USING ERRCODE = 'check_violation', HINT = 'post an adjustment instead';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER close_lock_split_tags
    BEFORE INSERT OR UPDATE OR DELETE ON split_tags
    FOR EACH ROW EXECUTE FUNCTION close_refuse_locked_split_tag();
