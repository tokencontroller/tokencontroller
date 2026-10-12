-- Whether the provider will no longer change a bill's figures (docs:
-- Integrations > Anthropic Enterprise; Reference > Bill schema). A pulled
-- bill can come in before it is final, and the close of its period waits
-- until it is. Bills sent from the API, uploaded or typed in are final.
ALTER TABLE bill_revisions ADD COLUMN final boolean NOT NULL DEFAULT true;
