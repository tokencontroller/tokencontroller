-- Other tickets a token entry could have been placed on: ticket keys the
-- session named, in the branch, a title or the prompts, that no split
-- landed on. Written each time the token entry is placed, so the review
-- list can offer them (docs: Attribute > Review and submit).
ALTER TABLE token_entries ADD COLUMN candidate_tickets text[] NOT NULL DEFAULT '{}';
