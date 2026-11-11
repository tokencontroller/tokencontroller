-- second step: no released version uses ticket_systems, ticket_system_projects or ticket_system_id since #156 reached production; every version since reads and writes the views tag_sources and tag_source_projects.
-- The views go and the tables take their names (ADR 0027, ADR 0028). The
-- version serving keeps working: the tables answer to the same names and
-- columns the views had.
DROP VIEW tag_source_projects;
DROP VIEW tag_sources;

ALTER TABLE ticket_systems RENAME TO tag_sources;
ALTER TABLE ticket_system_projects RENAME TO tag_source_projects;
ALTER TABLE tag_source_projects RENAME COLUMN ticket_system_id TO tag_source_id;

ALTER TABLE tag_sources RENAME CONSTRAINT ticket_systems_pkey TO tag_sources_pkey;
ALTER TABLE tag_sources RENAME CONSTRAINT ticket_systems_organization_id_id_key TO tag_sources_organization_id_id_key;
ALTER TABLE tag_sources RENAME CONSTRAINT ticket_systems_organization_id_fkey TO tag_sources_organization_id_fkey;
ALTER TABLE tag_sources RENAME CONSTRAINT ticket_systems_created_by_fkey TO tag_sources_created_by_fkey;
ALTER TABLE tag_sources RENAME CONSTRAINT ticket_systems_kind_check TO tag_sources_kind_check;
ALTER TABLE tag_source_projects RENAME CONSTRAINT ticket_system_projects_pkey TO tag_source_projects_pkey;
ALTER TABLE tag_source_projects
    RENAME CONSTRAINT ticket_system_projects_organization_id_ticket_system_id_fkey
    TO tag_source_projects_organization_id_tag_source_id_fkey;
