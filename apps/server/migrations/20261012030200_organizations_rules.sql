-- Organization rules and personal rules (docs: Attribute > Rules). A rule
-- list is a JSON array in the core crate's Rule shape, in order.

-- NULL rules means the default rules, so organizations that never changed
-- them get new defaults too.
ALTER TABLE organizations
    ADD COLUMN rules jsonb CHECK (rules IS NULL OR jsonb_typeof(rules) = 'array'),
    -- What the "ticket key in prompts" rule does when prompts name several
    -- tickets.
    ADD COLUMN several_keys text NOT NULL DEFAULT 'most_often'
        CHECK (several_keys IN ('most_often', 'equal_split', 'first_named')),
    -- Whether token entries no rule placed are matched to a ticket by their
    -- summary.
    ADD COLUMN summary_match boolean NOT NULL DEFAULT true;

-- A person's own rules. They run after the organization's.
ALTER TABLE people
    ADD COLUMN rules jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(rules) = 'array');
