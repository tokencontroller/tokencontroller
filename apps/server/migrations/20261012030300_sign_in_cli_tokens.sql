-- A local tool's session is the CLI token from `tc connect` (`tc_cli_`).
-- People list and revoke them under Settings, next to their API tokens, so
-- each session gets an ID and, for CLI tokens, the first characters of the
-- token to tell them apart.
ALTER TABLE sessions
    ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE,
    ADD COLUMN shown_prefix text;
