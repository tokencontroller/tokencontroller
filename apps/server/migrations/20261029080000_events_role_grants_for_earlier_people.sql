-- People who joined before cost model 2 kept their role but got no grant
-- events, and access reads only grants (ADR 0023). An org manager from then
-- could open the organization yet saw no message, session or tag. Write the
-- grants their role gives, as `grants::set_role_grants` does, for every
-- person whose role wrote none. A people manager's filters went with the
-- earlier tables, so their role has nothing to write; an org manager gives
-- them filters again.
WITH missing AS (
    SELECT p.organization_id, p.id, p.email, p.role
    FROM people p
    WHERE p.removed_at IS NULL
      AND NOT EXISTS (
          SELECT 1 FROM events e
          WHERE e.organization_id = p.organization_id
            AND e.kind = 'grant'
            AND starts_with(e.id, 'grt_role_' || replace(p.id::text, '-', '') || '_')
      )
),
role_grants AS (
    SELECT m.organization_id, m.id, m.email, c.capability, '{}'::jsonb AS filter
    FROM missing m
    CROSS JOIN (VALUES ('view'), ('assign'), ('approve'), ('plan'), ('grant'), ('adjust'), ('cutoff'))
        AS c (capability)
    WHERE m.role = 'org_manager'
    UNION ALL
    SELECT m.organization_id, m.id, m.email, 'view', jsonb_build_object('person', m.email)
    FROM missing m
    WHERE m.role = 'agent_manager'
)
INSERT INTO events (organization_id, id, kind, payload)
SELECT
    organization_id,
    'grt_role_' || replace(id::text, '-', '') || '_' || replace(gen_random_uuid()::text, '-', ''),
    'grant',
    jsonb_build_object('grant', jsonb_build_object(
        'person', email, 'capability', capability, 'filter', filter))
FROM role_grants;
