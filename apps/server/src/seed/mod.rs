//! Seed data: the example organization the docs use, loaded with one
//! command, for local development, CI, staging and screenshots.
//!
//! Kestrel Digital is a small agency: teams web, mobile and data, clients
//! Acme and Globex, the projects storefront (tickets `KD-…`), claims-portal
//! (`CP-…`), intranet and support. Mara Ortner (`m.ortner@kestrel.example`)
//! is the agent manager the docs show. Every record has a fixed ID in
//! [`ids`], so other areas' seed data can point at it.
//!
//! `tokencontroller seed` applies migrations and calls [`load`]. Staging runs
//! it after every reset, so [`load`] must be safe to run more than once: it
//! puts every seeded record back the way it is written here.
//!
//! To add another area's example data, write a `seed` function in that area
//! and call it at the end of [`load`].
//!
//! Owner: the service platform work.

use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::tags::{self, Tags};

/// Fixed IDs of the seeded records.
pub mod ids {
    use uuid::Uuid;

    pub const ORGANIZATION: Uuid = Uuid::from_u128(0x6b65_7374_0000_4000_8000_0000_0000_0001);

    pub const NADIA: Uuid = person(1);
    pub const PRIYA: Uuid = person(2);
    pub const KAI: Uuid = person(3);
    pub const MARA: Uuid = person(4);
    pub const JONAS: Uuid = person(5);
    pub const TOM: Uuid = person(6);
    pub const SAM: Uuid = person(7);
    pub const LENA: Uuid = person(8);
    pub const OLA: Uuid = person(9);

    pub const STOREFRONT: Uuid = project(1);
    pub const CLAIMS_PORTAL: Uuid = project(2);
    pub const INTRANET: Uuid = project(3);
    pub const SUPPORT: Uuid = project(4);

    pub const KD_200: Uuid = ticket(200);
    pub const KD_214: Uuid = ticket(214);
    pub const KD_231: Uuid = ticket(231);
    pub const KD_240: Uuid = ticket(240);
    pub const KD_260: Uuid = ticket(260);
    pub const CP_12: Uuid = ticket(1012);
    pub const CP_15: Uuid = ticket(1015);
    pub const INT_3: Uuid = ticket(2003);

    const fn person(n: u128) -> Uuid {
        Uuid::from_u128(0x6b65_7374_0000_4000_8001_0000_0000_0000 | n)
    }

    const fn project(n: u128) -> Uuid {
        Uuid::from_u128(0x6b65_7374_0000_4000_8002_0000_0000_0000 | n)
    }

    const fn ticket(n: u128) -> Uuid {
        Uuid::from_u128(0x6b65_7374_0000_4000_8003_0000_0000_0000 | n)
    }
}

/// One seeded person.
struct Person {
    id: Uuid,
    email: &'static str,
    name: &'static str,
    role: &'static str,
    tags: &'static [(&'static str, &'static str)],
    joined: bool,
}

const PEOPLE: [Person; 9] = [
    Person {
        id: ids::NADIA,
        email: "n.berger@kestrel.example",
        name: "Nadia Berger",
        role: "controller",
        tags: &[("department", "operations")],
        joined: true,
    },
    Person {
        id: ids::PRIYA,
        email: "p.shah@kestrel.example",
        name: "Priya Shah",
        role: "people_manager",
        tags: &[("team", "web"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::KAI,
        email: "k.novak@kestrel.example",
        name: "Kai Novak",
        role: "people_manager",
        tags: &[("department", "accounts")],
        joined: true,
    },
    Person {
        id: ids::MARA,
        email: "m.ortner@kestrel.example",
        name: "Mara Ortner",
        role: "agent_manager",
        tags: &[("team", "web"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::JONAS,
        email: "j.weber@kestrel.example",
        name: "Jonas Weber",
        role: "agent_manager",
        tags: &[("team", "web"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::TOM,
        email: "t.brandt@kestrel.example",
        name: "Tom Brandt",
        role: "people_manager",
        tags: &[("team", "mobile"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::SAM,
        email: "s.rossi@kestrel.example",
        name: "Sam Rossi",
        role: "agent_manager",
        tags: &[("team", "mobile"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::LENA,
        email: "l.hofer@kestrel.example",
        name: "Lena Hofer",
        role: "agent_manager",
        tags: &[("team", "data"), ("department", "engineering")],
        joined: true,
    },
    Person {
        id: ids::OLA,
        email: "o.nowak@kestrel.example",
        name: "Ola Nowak",
        role: "agent_manager",
        tags: &[("team", "data"), ("department", "engineering")],
        joined: false,
    },
];

/// The filters people managers read: Priya leads web, Tom leads mobile, Kai
/// manages the Acme account.
const FILTERS: [(Uuid, &str, &str); 3] = [
    (ids::PRIYA, "team", "web"),
    (ids::TOM, "team", "mobile"),
    (ids::KAI, "client", "acme"),
];

struct Project {
    id: Uuid,
    name: &'static str,
    pattern: Option<&'static str>,
    client: Option<&'static str>,
    repositories: &'static [&'static str],
}

const PROJECTS: [Project; 4] = [
    Project {
        id: ids::STOREFRONT,
        name: "storefront",
        pattern: Some(r"^KD-\d+"),
        client: Some("acme"),
        repositories: &["kestrel/storefront"],
    },
    Project {
        id: ids::CLAIMS_PORTAL,
        name: "claims-portal",
        pattern: Some(r"^CP-\d+"),
        client: Some("globex"),
        repositories: &["kestrel/claims-portal"],
    },
    Project {
        id: ids::INTRANET,
        name: "intranet",
        pattern: Some(r"^INT-\d+"),
        client: None,
        repositories: &["kestrel/intranet", "kestrel/sandbox"],
    },
    Project {
        id: ids::SUPPORT,
        name: "support",
        pattern: None,
        client: None,
        repositories: &[],
    },
];

struct Ticket {
    id: Uuid,
    key: &'static str,
    project: Uuid,
    title: &'static str,
    parent: Option<Uuid>,
    assignee: Option<Uuid>,
    source: &'static str,
}

const TICKETS: [Ticket; 8] = [
    Ticket {
        id: ids::KD_200,
        key: "KD-200",
        project: ids::STOREFRONT,
        title: "Checkout rebuild",
        parent: None,
        assignee: Some(ids::PRIYA),
        source: "jira",
    },
    Ticket {
        id: ids::KD_214,
        key: "KD-214",
        project: ids::STOREFRONT,
        title: "Fix checkout redirect loop",
        parent: Some(ids::KD_200),
        assignee: Some(ids::MARA),
        source: "jira",
    },
    Ticket {
        id: ids::KD_231,
        key: "KD-231",
        project: ids::STOREFRONT,
        title: "Saved addresses at checkout",
        parent: Some(ids::KD_200),
        assignee: Some(ids::JONAS),
        source: "jira",
    },
    Ticket {
        id: ids::KD_240,
        key: "KD-240",
        project: ids::STOREFRONT,
        title: "Address autocomplete",
        parent: Some(ids::KD_200),
        assignee: Some(ids::JONAS),
        source: "jira",
    },
    Ticket {
        id: ids::KD_260,
        key: "KD-260",
        project: ids::STOREFRONT,
        title: "Retry failed card payments",
        parent: None,
        assignee: Some(ids::MARA),
        source: "jira",
    },
    Ticket {
        id: ids::CP_12,
        key: "CP-12",
        project: ids::CLAIMS_PORTAL,
        title: "Claim upload form",
        parent: None,
        assignee: Some(ids::SAM),
        source: "jira",
    },
    Ticket {
        id: ids::CP_15,
        key: "CP-15",
        project: ids::CLAIMS_PORTAL,
        title: "Claim status emails",
        parent: None,
        assignee: Some(ids::LENA),
        source: "jira",
    },
    Ticket {
        id: ids::INT_3,
        key: "INT-3",
        project: ids::INTRANET,
        title: "Upgrade the build tooling",
        parent: None,
        assignee: Some(ids::JONAS),
        source: "manual",
    },
];

/// The invite link the docs show, `…/i/kestrel-4f2a`, belongs to Ola, who
/// has not signed in yet.
const INVITE_CODE: &str = "kestrel-4f2a";

/// Loads the example organization. Safe to run again on a seeded database.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    organization(&mut tx).await?;
    people(&mut tx).await?;
    projects(&mut tx).await?;
    tickets(&mut tx).await?;
    tx.commit().await?;
    tracing::info!("seed data loaded: Kestrel Digital");
    crate::ingest::seed::load(db).await?;
    Ok(())
}

async fn organization(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO organizations (id, name, slug, cost_source, reporting_currency, approval)
           VALUES ($1, 'Kestrel Digital', 'kestrel', 'usage_at_api_rates', 'USD', false)
           ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, slug = EXCLUDED.slug,
             cost_source = EXCLUDED.cost_source, reporting_currency = EXCLUDED.reporting_currency,
             approval = EXCLUDED.approval, sign_in_domains = '{}'"#,
        ids::ORGANIZATION
    )
    .execute(&mut *tx)
    .await?;
    tags::create_defaults(&mut *tx, ids::ORGANIZATION).await?;
    sqlx::query!(
        r#"INSERT INTO tag_names (organization_id, name)
           SELECT $1, name FROM unnest($2::text[]) AS name ON CONFLICT DO NOTHING"#,
        ids::ORGANIZATION,
        &["team", "client", "department", "purpose"].map(str::to_owned)
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn people(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    for person in &PEOPLE {
        sqlx::query!(
            r#"INSERT INTO people (id, organization_id, email, name, role, tags, joined_at, removed_at)
               VALUES ($1, $2, $3, $4, $5, $6, CASE WHEN $7 THEN now() END, NULL)
               ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name,
                 role = EXCLUDED.role, tags = EXCLUDED.tags, removed_at = NULL,
                 joined_at = CASE WHEN $7 THEN coalesce(people.joined_at, now()) END"#,
            person.id,
            ids::ORGANIZATION,
            person.email,
            person.name,
            person.role,
            Json(tags_of(person.tags)) as _,
            person.joined
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        "DELETE FROM people_manager_filters WHERE organization_id = $1",
        ids::ORGANIZATION
    )
    .execute(&mut *tx)
    .await?;
    for (person, name, value) in FILTERS {
        sqlx::query!(
            "INSERT INTO people_manager_filters (organization_id, person_id, filter) VALUES ($1, $2, $3)",
            ids::ORGANIZATION,
            person,
            Json(tags_of(&[(name, value)])) as _
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        r#"INSERT INTO invites (code, organization_id, person_id, created_by)
           VALUES ($1, $2, $3, $4) ON CONFLICT (code) DO NOTHING"#,
        INVITE_CODE,
        ids::ORGANIZATION,
        ids::OLA,
        ids::NADIA
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn projects(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    for project in &PROJECTS {
        let tags: Tags = project
            .client
            .map(|client| tags_of(&[("client", client)]))
            .unwrap_or_default();
        sqlx::query!(
            r#"INSERT INTO projects (id, organization_id, name, ticket_key_pattern, tags)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name,
                 ticket_key_pattern = EXCLUDED.ticket_key_pattern, tags = EXCLUDED.tags, archived_at = NULL"#,
            project.id,
            ids::ORGANIZATION,
            project.name,
            project.pattern,
            Json(tags) as _
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM project_repositories WHERE project_id = $1",
            project.id
        )
        .execute(&mut *tx)
        .await?;
        for repository in project.repositories {
            sqlx::query!(
                r#"INSERT INTO project_repositories (organization_id, repository, project_id)
                   VALUES ($1, $2, $3)
                   ON CONFLICT (organization_id, repository) DO UPDATE SET project_id = EXCLUDED.project_id"#,
                ids::ORGANIZATION,
                *repository,
                project.id
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    Ok(())
}

async fn tickets(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    // Parents first, so every parent exists when its children arrive.
    for ticket in TICKETS
        .iter()
        .filter(|ticket| ticket.parent.is_none())
        .chain(TICKETS.iter().filter(|ticket| ticket.parent.is_some()))
    {
        sqlx::query!(
            r#"INSERT INTO tickets (id, organization_id, key, project_id, title, parent_id, assignee_id, source)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (id) DO UPDATE SET key = EXCLUDED.key, project_id = EXCLUDED.project_id,
                 title = EXCLUDED.title, parent_id = EXCLUDED.parent_id,
                 assignee_id = EXCLUDED.assignee_id, source = EXCLUDED.source, updated_at = now()"#,
            ticket.id,
            ids::ORGANIZATION,
            ticket.key,
            ticket.project,
            ticket.title,
            ticket.parent,
            ticket.assignee,
            ticket.source
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

fn tags_of(pairs: &[(&str, &str)]) -> Tags {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_ids_are_all_different() {
        let mut all: Vec<Uuid> = PEOPLE.iter().map(|person| person.id).collect();
        all.extend(PROJECTS.iter().map(|project| project.id));
        all.extend(TICKETS.iter().map(|ticket| ticket.id));
        all.push(ids::ORGANIZATION);
        let count = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), count);
    }

    #[test]
    fn the_docs_agent_manager_is_seeded() {
        let mara = PEOPLE.iter().find(|person| person.id == ids::MARA).unwrap();
        assert_eq!(mara.email, "m.ortner@kestrel.example");
        assert_eq!(mara.role, "agent_manager");
    }
}
