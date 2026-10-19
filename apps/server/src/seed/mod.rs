//! Seed data: the example organization the docs use, loaded with one
//! command, for local development, CI, staging and screenshots.
//!
//! Kestrel Digital is a small agency. Its March 2026 is the worked month
//! of the docs (Concepts > The month): Mara and Tom on team web in
//! engineering, the project Shop for the client Acme with the features
//! KD-1 and KD-2, a bill of 1,000 for 1 to 31 March, four sessions that
//! cost 950, a plan of 750 over 100 days for Shop, an estimate of 500 for
//! KD-1, and Mara's share of 0.6 of Shop from 1 March. Nadia is the org
//! manager; Priya the people manager of team web. Every record has a fixed
//! id in [`ids`] and every event a fixed event id, so other areas' seed
//! data can point at them.
//!
//! `tokencontroller seed` applies migrations and calls [`load`]. Staging
//! runs it after every reset, so [`load`] must be safe to run more than
//! once: rows are put back the way they are written here, and events with
//! the same id are left as they are.
//!
//! Owner: the service platform work.

use sqlx::{PgConnection, PgPool};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};
use tokencontroller_core::{
    Accept, AttributeValue, Bill, DateRange, Distribution, Estimate, Event, EventId, Filter,
    Message, ModelRef, PersonAttribute, PriceRow, PriceSource, Record, Share, Split, SplitPart,
    SplitTarget, TagName, TagValue, TagValueRecord, Tags, TokenCounts, TokenKind,
};
use uuid::Uuid;

use crate::organizations::Role;
use crate::tags;

/// Fixed IDs of the seeded records.
pub mod ids {
    use uuid::Uuid;

    pub const ORGANIZATION: Uuid = Uuid::from_u128(0x6b65_7374_0000_4000_8000_0000_0000_0001);

    pub const NADIA: Uuid = person(1);
    pub const PRIYA: Uuid = person(2);
    pub const MARA: Uuid = person(4);
    pub const TOM: Uuid = person(6);
    pub const OLA: Uuid = person(9);

    const fn person(n: u128) -> Uuid {
        Uuid::from_u128(0x6b65_7374_0000_4000_8001_0000_0000_0000 | n)
    }
}

/// The emails the seeded people sign in with.
pub mod emails {
    pub const NADIA: &str = "n.varga@kestrel.example";
    pub const PRIYA: &str = "p.raman@kestrel.example";
    pub const MARA: &str = "m.ortner@kestrel.example";
    pub const TOM: &str = "t.lindqvist@kestrel.example";
    pub const OLA: &str = "o.berg@kestrel.example";
}

/// The provider and model every seeded message ran on, priced in the
/// seed's own price table row.
pub const PROVIDER: &str = "anthropic";
pub const MODEL: &str = "claude-sonnet-4-5";

/// One seeded person.
struct Person {
    id: Uuid,
    email: &'static str,
    name: &'static str,
    role: Role,
    /// The people manager's filters.
    filters: &'static [(&'static str, &'static str)],
    joined: bool,
}

const PEOPLE: [Person; 5] = [
    Person {
        id: ids::NADIA,
        email: emails::NADIA,
        name: "Nadia Varga",
        role: Role::OrgManager,
        filters: &[],
        joined: true,
    },
    Person {
        id: ids::PRIYA,
        email: emails::PRIYA,
        name: "Priya Raman",
        role: Role::PeopleManager,
        filters: &[("team", "web")],
        joined: true,
    },
    Person {
        id: ids::MARA,
        email: emails::MARA,
        name: "Mara Ortner",
        role: Role::AgentManager,
        filters: &[],
        joined: true,
    },
    Person {
        id: ids::TOM,
        email: emails::TOM,
        name: "Tom Lindqvist",
        role: Role::AgentManager,
        filters: &[],
        joined: true,
    },
    Person {
        id: ids::OLA,
        email: emails::OLA,
        name: "Ola Berg",
        role: Role::AgentManager,
        filters: &[],
        joined: false,
    },
];

/// The invite link the docs show, `…/i/kestrel-4f2a`, belongs to Ola, who
/// has not signed in yet.
const INVITE_CODE: &str = "kestrel-4f2a";

/// The days of March 2026.
const MARCH: DateRange = DateRange {
    from: date!(2026 - 03 - 01),
    to: date!(2026 - 03 - 31),
};

/// Loads the example organization. Safe to run again on a seeded database.
///
/// # Errors
///
/// Fails if a query fails or an event cannot be built.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    organization(&mut tx).await?;
    people(&mut tx).await?;
    for event in events()? {
        put(&mut tx, &event).await?;
    }
    sqlx::query!(
        "INSERT INTO bill_details (organization_id, event_id, source, is_final, lines)
         VALUES ($1, $2, 'api', true, '[]'::jsonb) ON CONFLICT DO NOTHING",
        ids::ORGANIZATION,
        "bil_seed_anthropic_2026_03"
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!("seed data loaded: Kestrel Digital");
    Ok(())
}

async fn organization(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO organizations (id, name, slug, reporting_currency, approval)
           VALUES ($1, 'Kestrel Digital', 'kestrel', 'USD', false)
           ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, slug = EXCLUDED.slug,
             reporting_currency = EXCLUDED.reporting_currency,
             approval = EXCLUDED.approval, sign_in_domains = '{}'"#,
        ids::ORGANIZATION
    )
    .execute(&mut *tx)
    .await?;
    tags::create_defaults(&mut *tx, ids::ORGANIZATION).await?;
    Ok(())
}

async fn people(tx: &mut PgConnection) -> Result<(), sqlx::Error> {
    for person in &PEOPLE {
        sqlx::query!(
            r#"INSERT INTO people (id, organization_id, email, name, role, joined_at, removed_at)
               VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN now() END, NULL)
               ON CONFLICT (id) DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name,
                 role = EXCLUDED.role, removed_at = NULL,
                 joined_at = CASE WHEN $6 THEN coalesce(people.joined_at, now()) END"#,
            person.id,
            ids::ORGANIZATION,
            person.email,
            person.name,
            person.role.as_str(),
            person.joined
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

/// Writes one event unless one with its id is in the log already.
async fn put(tx: &mut PgConnection, event: &Event) -> Result<(), sqlx::Error> {
    let payload = serde_json::to_value(&event.record).unwrap_or_default();
    sqlx::query!(
        "INSERT INTO events (organization_id, id, recorded_at, recorded_by, supersedes, kind, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT DO NOTHING",
        ids::ORGANIZATION,
        event.id.as_str(),
        event.recorded_at,
        event.recorded_by.as_ref().map(TagValue::as_str),
        event.supersedes.as_ref().map(EventId::as_str),
        event.record.kind(),
        payload
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

fn person(email: &str) -> anyhow::Result<TagValue> {
    Ok(TagValue::new(email)?)
}

fn value(text: &str) -> anyhow::Result<TagValue> {
    Ok(TagValue::new(text)?)
}

fn name(text: &str) -> anyhow::Result<TagName> {
    Ok(TagName::new(text)?)
}

fn tags(pairs: &[(&str, &str)]) -> anyhow::Result<Tags> {
    let mut tags = Tags::new();
    for (tag, key) in pairs {
        tags.insert(name(tag)?, value(key)?);
    }
    Ok(tags)
}

fn usd(text: &str) -> anyhow::Result<tokencontroller_core::Money> {
    Ok(tokencontroller_core::Money::parse(
        text,
        tokencontroller_core::Currency::USD,
    )?)
}

fn event(id: &str, at: OffsetDateTime, record: Record) -> anyhow::Result<Event> {
    Ok(Event::new(id, at, record)?)
}

/// One session of the worked month: one message of `input_millions`
/// million input tokens, which at the seed's price of 10 USD per million
/// costs `input_millions` × 10 USD.
struct Session<'a> {
    id: &'a str,
    key: &'a str,
    person: &'a str,
    at: OffsetDateTime,
    input_millions: u64,
    agent_name: Option<&'a str>,
    tags: Tags,
    summary: &'a str,
}

fn message(session: Session<'_>) -> anyhow::Result<Event> {
    let record = Message {
        session_id: tokencontroller_core::SessionId::new(session.key)?,
        message_id: tokencontroller_core::MessageId::new("1")?,
        person: person(session.person)?,
        model: ModelRef::new(PROVIDER, MODEL),
        tokens: Some(TokenCounts {
            input: session.input_millions.saturating_mul(1_000_000),
            output: 0,
            cache_write: 0,
            cache_read: 0,
        }),
        at: session.at,
        agent_harness: "claude-code".to_owned(),
        agent_name: session.agent_name.map(str::to_owned),
        reported_cost: None,
        tags: session.tags,
        signals: tokencontroller_core::Signals::default(),
        session: tokencontroller_core::SessionFacts {
            turns: Some(12),
            tool_calls: Some(30),
            summary: Some(session.summary.to_owned()),
            ..Default::default()
        },
    };
    Ok(
        event(session.id, session.at, Record::Message(Box::new(record)))?
            .by(person(session.person)?),
    )
}

/// The role grants of a seeded person, with ids the grants area reads as
/// role grants.
fn role_grants(person: &Person) -> anyhow::Result<Vec<Event>> {
    let who = self::person(person.email)?;
    let filters: Vec<Filter> = person
        .filters
        .iter()
        .map(|(tag, key)| Ok(Filter(tags(&[(tag, key)])?)))
        .collect::<anyhow::Result<_>>()?;
    let prefix = crate::grants::role_prefix(person.id);
    person
        .role
        .core()
        .grants(&who, &filters)
        .into_iter()
        .enumerate()
        .map(|(index, grant)| {
            event(
                &format!("{prefix}seed{index}"),
                datetime!(2026-02-01 09:00 UTC),
                Record::Grant(grant),
            )
        })
        .collect()
}

/// Every event of the worked month, in the order recorded.
///
/// # Errors
///
/// Never in practice; every id and amount here fits.
pub fn events() -> anyhow::Result<Vec<Event>> {
    let setup = datetime!(2026-02-01 09:00 UTC);
    let nadia = person(emails::NADIA)?;
    let mara = person(emails::MARA)?;
    let mut events = setup_events(setup, &nadia)?;
    events.extend(plan_events(&nadia, &mara)?);
    events.extend(month_events(&nadia, &mara)?);
    Ok(events)
}

/// The organization as it stands before the month: role grants, the price
/// row, attributes, people on teams and the tag values.
fn setup_events(setup: OffsetDateTime, nadia: &TagValue) -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();
    for person in &PEOPLE {
        events.extend(role_grants(person)?);
    }

    // The price table: one row of the organization's own, so the month
    // costs the same on every instance.
    events.push(event(
        "prc_seed_sonnet_input",
        setup,
        Record::PriceRow(PriceRow {
            provider: PROVIDER.to_owned(),
            model: MODEL.to_owned(),
            kind: TokenKind::Input,
            per_million: usd("10")?,
            valid_from: date!(2026 - 01 - 01),
            source: PriceSource::Organization,
        }),
    )?);

    // Attributes: team web in department engineering; Mara and Tom on it.
    events.push(
        event(
            "att_seed_department_engineering",
            setup,
            Record::AttributeValue(AttributeValue {
                name: name("department")?,
                key: value("engineering")?,
                title: Some("Engineering".to_owned()),
                defaults: Tags::new(),
            }),
        )?
        .by(nadia.clone()),
    );
    events.push(
        event(
            "att_seed_team_web",
            setup,
            Record::AttributeValue(AttributeValue {
                name: name("team")?,
                key: value("web")?,
                title: Some("Web".to_owned()),
                defaults: tags(&[("department", "engineering")])?,
            }),
        )?
        .by(nadia.clone()),
    );
    for (id, email) in [
        ("pat_seed_mara_web", emails::MARA),
        ("pat_seed_tom_web", emails::TOM),
    ] {
        events.push(
            event(
                id,
                setup,
                Record::PersonAttribute(PersonAttribute {
                    person: person(email)?,
                    name: name("team")?,
                    key: value("web")?,
                    from: Some(date!(2026 - 03 - 01)),
                }),
            )?
            .by(nadia.clone()),
        );
    }
    events.extend(value_events(setup, nadia)?);
    Ok(events)
}

/// The tag values on record: the client Acme, the project Shop and the
/// features KD-1 and KD-2.
fn value_events(setup: OffsetDateTime, nadia: &TagValue) -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();
    // Tag values: the client Acme, the project Shop, the features.
    let tag_value = |id: &str, record: TagValueRecord| -> anyhow::Result<Event> {
        Ok(event(id, setup, Record::TagValue(record))?.by(nadia.clone()))
    };
    events.push(tag_value(
        "tag_seed_client_acme",
        TagValueRecord {
            name: TagName::client(),
            key: value("acme")?,
            title: Some("Acme".to_owned()),
            description: None,
            source_id: None,
            source_status: None,
            defaults: Tags::new(),
        },
    )?);
    events.push(tag_value(
        "tag_seed_project_shop",
        TagValueRecord {
            name: TagName::project(),
            key: value("shop")?,
            title: Some("Shop".to_owned()),
            description: Some("Acme's web shop.".to_owned()),
            source_id: None,
            source_status: None,
            defaults: tags(&[("client", "acme")])?,
        },
    )?);
    for (id, key, title, description) in [
        (
            "tag_seed_feature_kd_1",
            "KD-1",
            "Checkout redesign",
            "A new checkout flow with saved addresses and one-page payment.",
        ),
        (
            "tag_seed_feature_kd_2",
            "KD-2",
            "Returns portal",
            "Customers start a return and print the label themselves.",
        ),
    ] {
        events.push(tag_value(
            id,
            TagValueRecord {
                name: TagName::feature(),
                key: value(key)?,
                title: Some(title.to_owned()),
                description: Some(description.to_owned()),
                source_id: None,
                source_status: None,
                defaults: tags(&[("project", "shop")])?,
            },
        )?);
    }
    Ok(events)
}

/// The plan: the estimates, what was accepted, and Mara's share of Shop.
fn plan_events(nadia: &TagValue, mara: &TagValue) -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();
    // The plan: Shop, 750 over 100 days from 1 March; KD-1, 500 in March.
    events.push(
        event(
            "est_seed_shop",
            datetime!(2026-02-20 10:00 UTC),
            Record::Estimate(Estimate {
                description: "Shop: checkout redesign and returns portal".to_owned(),
                tags: tags(&[("project", "shop")])?,
                distribution: Distribution {
                    median: usd("750")?,
                    bands: Vec::new(),
                },
                markup_basis_points: 0,
                estimator: emails::NADIA.to_owned(),
                range: DateRange::new(date!(2026 - 03 - 01), date!(2026 - 06 - 08))?,
            }),
        )?
        .by(nadia.clone()),
    );
    events.push(
        event(
            "acc_seed_shop",
            datetime!(2026-02-20 10:05 UTC),
            Record::Accept(Accept {
                estimate: EventId::new("est_seed_shop")?,
            }),
        )?
        .by(nadia.clone()),
    );
    events.push(
        event(
            "est_seed_kd_1",
            datetime!(2026-02-21 10:00 UTC),
            Record::Estimate(Estimate {
                description: "Checkout redesign".to_owned(),
                tags: tags(&[("feature", "KD-1")])?,
                distribution: Distribution {
                    median: usd("500")?,
                    bands: Vec::new(),
                },
                markup_basis_points: 0,
                estimator: "classifier".to_owned(),
                range: MARCH,
            }),
        )?
        .by(mara.clone()),
    );
    events.push(
        event(
            "acc_seed_kd_1",
            datetime!(2026-02-21 10:05 UTC),
            Record::Accept(Accept {
                estimate: EventId::new("est_seed_kd_1")?,
            }),
        )?
        .by(nadia.clone()),
    );
    events.push(
        event(
            "shr_seed_mara_shop",
            datetime!(2026-02-22 10:00 UTC),
            Record::Share(Share {
                person: mara.clone(),
                project: value("shop")?,
                fraction_basis_points: 6_000,
                from: date!(2026 - 03 - 01),
                to: None,
            }),
        )?
        .by(nadia.clone()),
    );
    Ok(events)
}

/// The month itself: the four sessions, the split and the bill.
fn month_events(nadia: &TagValue, mara: &TagValue) -> anyhow::Result<Vec<Event>> {
    let mut events = Vec::new();
    // The month's sessions: S1 to S4 of the worked month.
    events.push(message(Session {
        id: "msg_seed_s1",
        key: "seed-s1",
        person: emails::MARA,
        at: datetime!(2026-03-04 10:00 UTC),
        input_millions: 40,
        agent_name: None,
        tags: tags(&[("feature", "KD-1")])?,
        summary: "Checkout: saved addresses",
    })?);
    events.push(message(Session {
        id: "msg_seed_s2",
        key: "seed-s2",
        person: emails::MARA,
        at: datetime!(2026-03-12 14:00 UTC),
        input_millions: 30,
        agent_name: None,
        tags: Tags::new(),
        summary: "Shared payment component for checkout and returns",
    })?);
    events.push(
        event(
            "spl_seed_s2",
            datetime!(2026-03-12 16:00 UTC),
            Record::Split(Split {
                target: SplitTarget::Session(tokencontroller_core::SessionId::new("seed-s2")?),
                parts: vec![
                    SplitPart {
                        weight: 60,
                        tags: tags(&[("feature", "KD-1")])?,
                    },
                    SplitPart {
                        weight: 40,
                        tags: tags(&[("feature", "KD-2")])?,
                    },
                ],
            }),
        )?
        .by(mara.clone()),
    );
    events.push(message(Session {
        id: "msg_seed_s3",
        key: "seed-s3",
        person: emails::TOM,
        at: datetime!(2026-03-18 09:00 UTC),
        input_millions: 10,
        agent_name: Some("review bot"),
        tags: tags(&[("project", "shop")])?,
        summary: "Nightly review of open pull requests",
    })?);
    events.push(message(Session {
        id: "msg_seed_s4",
        key: "seed-s4",
        person: emails::TOM,
        at: datetime!(2026-03-25 11:00 UTC),
        input_millions: 15,
        agent_name: None,
        tags: Tags::new(),
        summary: "Exploring a logging library",
    })?);

    // The bill for March: 1,000, of which 50 no message explains.
    events.push(
        event(
            "bil_seed_anthropic_2026_03",
            datetime!(2026-04-02 08:00 UTC),
            Record::Bill(Bill {
                provider: PROVIDER.to_owned(),
                range: MARCH,
                amount: usd("1000")?,
            }),
        )?
        .by(nadia.clone()),
    );
    Ok(events)
}

/// The first day of the worked month.
#[must_use]
pub const fn month_start() -> Date {
    MARCH.from
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::{Capability, Currency, State};

    fn fold() -> State {
        let events = events().unwrap();
        State::fold(&events, datetime!(2026-04-10 00:00 UTC), Currency::USD).unwrap()
    }

    #[test]
    fn the_worked_month_adds_up() {
        let fold = fold();
        let everything = Filter::default();
        assert_eq!(
            fold.cost(&everything, MARCH).unwrap().amount,
            usd("950").unwrap()
        );
        let kd_1 = Filter(tags(&[("feature", "KD-1")]).unwrap());
        assert_eq!(fold.cost(&kd_1, MARCH).unwrap().amount, usd("580").unwrap());
        let shop = Filter(tags(&[("project", "shop")]).unwrap());
        assert_eq!(fold.cost(&shop, MARCH).unwrap().amount, usd("800").unwrap());
        let acme = Filter(tags(&[("client", "acme")]).unwrap());
        assert_eq!(fold.cost(&acme, MARCH).unwrap().amount, usd("800").unwrap());
        let web = Filter(tags(&[("team", "web")]).unwrap());
        assert_eq!(fold.cost(&web, MARCH).unwrap().amount, usd("950").unwrap());
        let engineering = Filter(tags(&[("department", "engineering")]).unwrap());
        assert_eq!(
            fold.cost(&engineering, MARCH).unwrap().amount,
            usd("950").unwrap()
        );
    }

    #[test]
    fn the_plan_the_share_and_the_bill_follow_the_docs() {
        let fold = fold();
        let shop = Filter(tags(&[("project", "shop")]).unwrap());
        assert_eq!(
            fold.plan(&shop, MARCH).unwrap(),
            Some(usd("232.50").unwrap())
        );
        let mara = Filter(tags(&[("person", emails::MARA)]).unwrap());
        assert_eq!(
            fold.plan(&mara, MARCH).unwrap(),
            Some(usd("139.50").unwrap())
        );
        let kd_1 = Filter(tags(&[("feature", "KD-1")]).unwrap());
        let deviation = fold.deviation(&kd_1, MARCH).unwrap().unwrap();
        assert_eq!(deviation.delta, usd("-80").unwrap());
        let bill = fold.bills().next().unwrap();
        let reconciliation = fold.reconcile(&bill.value).unwrap();
        assert_eq!(reconciliation.unexplained, usd("50").unwrap());
        assert!(!reconciliation.a_message_is_wrong());
    }

    #[test]
    fn the_seeded_people_hold_their_roles() {
        let fold = fold();
        let nadia = person(emails::NADIA).unwrap();
        let priya = person(emails::PRIYA).unwrap();
        let mara = person(emails::MARA).unwrap();
        assert!(fold.can(&nadia, Capability::Grant, &Filter::default()));
        let web = Filter(tags(&[("team", "web")]).unwrap());
        assert!(fold.can(&priya, Capability::Approve, &web));
        assert!(!fold.can(&priya, Capability::Approve, &Filter::default()));
        let own = Filter(tags(&[("person", emails::MARA)]).unwrap());
        assert!(fold.can(&mara, Capability::View, &own));
        assert!(!fold.can(&mara, Capability::View, &Filter::default()));
        assert_eq!(month_start(), date!(2026 - 03 - 01));
    }
}
