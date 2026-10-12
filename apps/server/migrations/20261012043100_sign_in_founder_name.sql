-- The founder's own name, given at sign-up. The organization's first member
-- gets it once the link is used. Docs: Reference > HTTP API.
ALTER TABLE sign_in_links ADD COLUMN sign_up_person_name text;
