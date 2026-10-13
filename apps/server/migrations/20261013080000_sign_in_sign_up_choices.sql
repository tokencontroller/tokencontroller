-- The answers to the sign-up questions travel with the link, so the
-- organization is set up the same in whichever browser opens it. They are
-- applied when the link creates the organization. Docs: Reference > HTTP API.
ALTER TABLE sign_in_links
    ADD COLUMN sign_up_cost_source text,
    ADD COLUMN sign_up_reporting_currency text,
    -- `agents` or `people`: picks the first steps the web app shows.
    ADD COLUMN sign_up_manages text;
