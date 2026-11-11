-- Jira and any later list that brings tag values is a tag source (ADR 0028);
-- "ticket system" is no longer a word of the product. The tables follow in
-- two releases (ADR 0027). This first step adds the new names as views over
-- the old tables, and the service reads and writes only the views. Both
-- views are simple, so Postgres writes through them, ON CONFLICT included.
-- The version before keeps the old table names. The second step, once no
-- released version uses the old names, drops the views and renames the
-- tables and the column to these names.
CREATE VIEW tag_sources AS
SELECT
    id,
    organization_id,
    kind,
    site,
    sealed_credentials,
    key_hint,
    created_by,
    created_at,
    sync_requested_at,
    last_sync_started_at,
    last_sync_at,
    last_error
FROM ticket_systems;

CREATE VIEW tag_source_projects AS
SELECT
    organization_id,
    ticket_system_id AS tag_source_id,
    external_project,
    external_name,
    project,
    fields,
    synced_until
FROM ticket_system_projects;
