//! Seed data for data coming in: the example organization's token entries
//! for last month and this month so far, so staging, screenshots and local
//! development show real-looking cost.
//!
//! Every workday, Mara and Jonas work on the storefront, Sam and Lena on the
//! claims portal, with Claude Code and Codex; the review bot looks at pull
//! requests and the support desk answers email. Each session has a fixed ID,
//! so loading again puts it back the way it is written here. Token entries in
//! a closed period are left as they are.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sqlx::PgPool;
use time::{Date, Duration, Month, OffsetDateTime, Time, Weekday};
use uuid::Uuid;

use super::entry::NewTokenEntry;
use super::store::{self, Door, StoreError};
use crate::seed::ids;

/// One kind of daily work in the example organization.
struct Work {
    /// Who the token entries belong to: their person, or for a bot, its
    /// agent manager.
    person: Uuid,
    /// Short name for session IDs.
    slug: &'static str,
    agent_harness: &'static str,
    agent_name: Option<&'static str>,
    model: &'static str,
    /// The hour the first session starts, UTC.
    hour: u8,
    /// Tickets worked on, in turn, with the branch name after the key.
    tickets: &'static [(&'static str, &'static str)],
    repository: Option<&'static str>,
    /// For work without tickets: the project.
    project: Option<&'static str>,
    summary: &'static str,
    /// Sessions per workday.
    sessions: u8,
    /// Input tokens of an average session.
    input: u64,
}

const WORK: [Work; 6] = [
    Work {
        person: ids::MARA,
        slug: "mara",
        agent_harness: "claude-code",
        agent_name: None,
        model: "claude-sonnet-5",
        hour: 8,
        tickets: &[
            ("KD-214", "checkout-redirect-loop"),
            ("KD-260", "retry-card-payments"),
        ],
        repository: Some("github.com/kestrel/storefront"),
        project: None,
        summary: "Worked on the checkout",
        sessions: 2,
        input: 42_000,
    },
    Work {
        person: ids::JONAS,
        slug: "jonas",
        agent_harness: "claude-code",
        agent_name: None,
        model: "claude-opus-5",
        hour: 9,
        tickets: &[
            ("KD-231", "saved-addresses"),
            ("KD-240", "address-autocomplete"),
            ("INT-3", "build-tooling"),
        ],
        repository: Some("github.com/kestrel/storefront"),
        project: None,
        summary: "Built the address features",
        sessions: 2,
        input: 36_000,
    },
    Work {
        person: ids::SAM,
        slug: "sam",
        agent_harness: "claude-code",
        agent_name: None,
        model: "claude-sonnet-5",
        hour: 7,
        tickets: &[("CP-12", "claim-upload-form")],
        repository: Some("github.com/kestrel/claims-portal"),
        project: None,
        summary: "Built the claim upload form",
        sessions: 1,
        input: 51_000,
    },
    Work {
        person: ids::LENA,
        slug: "lena",
        agent_harness: "codex-cli",
        agent_name: None,
        model: "gpt-5-codex",
        hour: 10,
        tickets: &[("CP-15", "claim-status-emails")],
        repository: Some("github.com/kestrel/claims-portal"),
        project: None,
        summary: "Wrote the claim status emails",
        sessions: 1,
        input: 64_000,
    },
    Work {
        person: ids::PRIYA,
        slug: "review-bot",
        agent_harness: "github-action",
        agent_name: Some("review bot"),
        model: "claude-sonnet-5",
        hour: 13,
        tickets: &[
            ("KD-231", "saved-addresses"),
            ("KD-214", "checkout-redirect-loop"),
        ],
        repository: Some("github.com/kestrel/storefront"),
        project: None,
        summary: "Reviewed a pull request",
        sessions: 1,
        input: 18_000,
    },
    Work {
        person: ids::NADIA,
        slug: "support-desk",
        agent_harness: "support-desk",
        agent_name: Some("email answers"),
        model: "claude-haiku-5",
        hour: 6,
        tickets: &[],
        repository: None,
        project: Some("support"),
        summary: "Answered customer email",
        sessions: 3,
        input: 6_000,
    },
];

/// Loads the token entries of last month and this month up to now.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn load(db: &PgPool) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let pricing = crate::integrations::prices::current_pricing(db).await?;
    let mut by_person: BTreeMap<Uuid, Vec<NewTokenEntry>> = BTreeMap::new();
    let mut day = first_of_last_month(now.date());
    while day <= now.date() {
        if !matches!(day.weekday(), Weekday::Saturday | Weekday::Sunday) {
            for (person, entry) in entries_of(day, now) {
                by_person.entry(person).or_default().push(entry);
            }
        }
        let Some(next) = day.next_day() else {
            break;
        };
        day = next;
    }
    let mut stored = 0_usize;
    for (person, entries) in by_person {
        stored = stored.saturating_add(store_all(db, person, entries, &pricing).await?);
    }
    tracing::info!(token_entries = stored, "seed data loaded: token entries");
    Ok(())
}

fn first_of_last_month(today: Date) -> Date {
    let (year, month) = match today.month() {
        Month::January => (today.year().saturating_sub(1), Month::December),
        month => (today.year(), month.previous()),
    };
    Date::from_calendar_date(year, month, 1).unwrap_or(today)
}

/// Stores one person's seeded token entries in one go, or one by one when
/// some are in a closed period, leaving those as they are. Returns how many
/// were stored.
async fn store_all(
    db: &PgPool,
    person: Uuid,
    entries: Vec<NewTokenEntry>,
    pricing: &store::Pricing,
) -> Result<usize, StoreError> {
    let count = entries.len();
    match store_some(db, person, entries.clone(), pricing).await {
        Err(StoreError::Locked { .. }) => {}
        other => return other.map(|()| count),
    }
    let mut stored = 0_usize;
    for entry in entries {
        match store_some(db, person, vec![entry], pricing).await {
            Ok(()) => stored = stored.saturating_add(1),
            Err(StoreError::Locked { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(stored)
}

async fn store_some(
    db: &PgPool,
    person: Uuid,
    entries: Vec<NewTokenEntry>,
    pricing: &store::Pricing,
) -> Result<(), StoreError> {
    let mut tx = db.begin().await?;
    store::store_token_entries(
        &mut tx,
        ids::ORGANIZATION,
        person,
        entries,
        Door::Endpoint,
        pricing,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The token entries of one workday that have ended by `now`.
fn entries_of(day: Date, now: OffsetDateTime) -> Vec<(Uuid, NewTokenEntry)> {
    let day_number = u64::from(day.ordinal());
    let mut entries = Vec::new();
    for work in &WORK {
        for session in 0..work.sessions {
            let turn = day_number.wrapping_add(u64::from(session));
            let Some(started) = start_of(day, work.hour, session) else {
                continue;
            };
            let length = Duration::minutes(i64::try_from(turn.wrapping_mul(7) % 50).unwrap_or(0))
                .saturating_add(Duration::minutes(12));
            let ended = started.saturating_add(length);
            if ended > now {
                continue;
            }
            let value = entry_json(work, day, session, turn, started, ended);
            if let Ok(entry) = serde_json::from_value::<NewTokenEntry>(value) {
                entries.push((work.person, entry));
            }
        }
    }
    entries
}

fn start_of(day: Date, hour: u8, session: u8) -> Option<OffsetDateTime> {
    let hour = hour.checked_add(session.checked_mul(3)?)?;
    Some(day.with_time(Time::from_hms(hour, 5, 0).ok()?).assume_utc())
}

fn entry_json(
    work: &Work,
    day: Date,
    session: u8,
    turn: u64,
    started: OffsetDateTime,
    ended: OffsetDateTime,
) -> Value {
    // Between 60 % and 140 % of the average, the same on every load.
    let percent = 60_u64.wrapping_add(turn.wrapping_mul(37) % 81);
    let input = work.input.saturating_mul(percent) / 100;
    let tokens = Tokens {
        input,
        output: input / 9,
        cache_write: input / 4,
        cache_read: input.saturating_mul(6),
    };
    let ticket = usize::try_from(turn)
        .ok()
        .and_then(|turn| turn.checked_rem(work.tickets.len()))
        .and_then(|index| work.tickets.get(index));
    let branch = ticket.map(|(key, name)| format!("{key}-{name}"));
    let session_id = format!("seed-{}-{day}-{session}", work.slug);
    let mut entry = json!({
        "agent_harness": work.agent_harness,
        "model": work.model,
        "started_at": started.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        "ended_at": ended.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
        "tokens": {
            "input": tokens.input,
            "output": tokens.output,
            "cache_write": tokens.cache_write,
            "cache_read": tokens.cache_read
        },
        "reported_cost": reported_cost(work.model, &tokens),
        "summary": work.summary,
        "session_id": session_id,
        "local_id": format!("{}:{session_id}", work.agent_harness),
        "signals": {
            "branch": branch,
            "repository": work.repository,
            "pull_request": (work.agent_harness == "github-action").then(|| (turn % 40).wrapping_add(101))
        }
    });
    if let (Some(object), Some(name)) = (entry.as_object_mut(), work.agent_name) {
        object.insert("agent_name".to_owned(), json!(name));
    }
    if let (Some(object), Some(project)) = (entry.as_object_mut(), work.project) {
        object.insert("project".to_owned(), json!(project));
    }
    entry
}

struct Tokens {
    input: u64,
    output: u64,
    cache_write: u64,
    cache_read: u64,
}

/// What the agent would report, in US dollars, from published prices per
/// million tokens: input, output, cache write and cache read.
fn reported_cost(model: &str, tokens: &Tokens) -> String {
    // Nanodollars per token.
    let (input, output, cache_write, cache_read): (u64, u64, u64, u64) = match model {
        "claude-opus-5" => (5_000, 25_000, 6_250, 500),
        "claude-haiku-5" => (1_000, 5_000, 1_250, 100),
        "gpt-5-codex" => (1_250, 10_000, 0, 125),
        _ => (3_000, 15_000, 3_750, 300),
    };
    let nanos = tokens
        .input
        .saturating_mul(input)
        .saturating_add(tokens.output.saturating_mul(output))
        .saturating_add(tokens.cache_write.saturating_mul(cache_write))
        .saturating_add(tokens.cache_read.saturating_mul(cache_read));
    format!("{}.{:09}", nanos / 1_000_000_000, nanos % 1_000_000_000)
}

#[cfg(test)]
mod tests {
    use time::macros::{date, datetime};

    use super::*;

    #[test]
    fn last_month_starts_on_its_first_day() {
        assert_eq!(
            first_of_last_month(date!(2026 - 10 - 12)),
            date!(2026 - 09 - 01)
        );
        assert_eq!(
            first_of_last_month(date!(2026 - 01 - 31)),
            date!(2025 - 12 - 01)
        );
    }

    #[test]
    fn a_workday_has_the_same_entries_on_every_load() {
        let day = date!(2026 - 10 - 05);
        let now = datetime!(2026-10-06 00:00 UTC);
        let first = entries_of(day, now);
        let second = entries_of(day, now);
        assert_eq!(first.len(), 10);
        assert_eq!(first.len(), second.len());
        for ((person, entry), (again_person, again)) in first.iter().zip(second.iter()) {
            assert_eq!(person, again_person);
            assert_eq!(entry.session_id, again.session_id);
            assert_eq!(entry.tokens, again.tokens);
            assert!(entry.clone().check().is_ok(), "{:?}", entry.session_id);
        }
    }

    #[test]
    fn sessions_that_have_not_ended_are_left_out() {
        let day = date!(2026 - 10 - 05);
        let morning = entries_of(day, datetime!(2026-10-05 08:00 UTC));
        assert!(morning.len() < entries_of(day, datetime!(2026-10-06 00:00 UTC)).len());
        assert!(entries_of(day, datetime!(2026-10-05 05:00 UTC)).is_empty());
    }

    #[test]
    fn reported_cost_is_written_in_dollars() {
        let tokens = Tokens {
            input: 1_000_000,
            output: 100_000,
            cache_write: 0,
            cache_read: 0,
        };
        assert_eq!(reported_cost("claude-sonnet-5", &tokens), "4.500000000");
        assert_eq!(reported_cost("gpt-5-codex", &tokens), "2.250000000");
    }
}
