-- Team plan subscriptions through Stripe, on the hosted service only. An
-- organization without an active subscription is on the free Solo plan.

CREATE TABLE billing_subscriptions (
    organization_id uuid PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    stripe_customer_id text NOT NULL,
    stripe_subscription_id text,
    -- Stripe's subscription status, such as `active`, `past_due` or
    -- `canceled`; NULL until the subscription exists.
    status text,
    -- `month` or `year`.
    billing_interval text CHECK (billing_interval IN ('month', 'year')),
    current_period_end timestamptz,
    cancel_at_period_end boolean NOT NULL DEFAULT false,
    -- When Stripe created the newest event applied here, so an older event
    -- that arrives late never overwrites a newer state.
    last_event_at timestamptz NOT NULL DEFAULT '-infinity',
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX billing_subscriptions_customer ON billing_subscriptions (stripe_customer_id);
