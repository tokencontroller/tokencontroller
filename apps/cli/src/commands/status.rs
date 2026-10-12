//! `tc status`: organization, person, agents found, rules, and today's cost
//! and share on tickets.

use serde::Serialize;

use crate::amounts::{decimal, display};
use crate::app::{App, day_of, start_of_day_ms};
use crate::entries::CURRENCY;
use crate::error::Result;
use crate::output::{self, fraction, percent};
use crate::store::Status;

#[derive(Debug, Serialize)]
struct Agents {
    agent_harness: String,
    sessions: u64,
}

#[derive(Debug, Serialize)]
struct Today {
    cost: String,
    share_on_tickets: Box<serde_json::value::RawValue>,
    #[serde(skip)]
    nanos: i64,
    #[serde(skip)]
    percent: u64,
}

/// The token sheet being filled: the drafts not yet submitted, and the
/// cadence period when an organization sets one.
#[derive(Debug, Serialize)]
struct Sheet {
    token_entries: usize,
    cost: String,
    share_on_tickets: Box<serde_json::value::RawValue>,
    period_start: Option<String>,
    period_end: Option<String>,
    due_at: Option<String>,
    is_late: bool,
}

#[derive(Debug, Serialize)]
struct View {
    connected: bool,
    service: Option<String>,
    organization: Option<String>,
    person: Option<String>,
    currency: String,
    agents: Vec<Agents>,
    rules: usize,
    today: Today,
    token_sheet: Sheet,
}

pub fn run(app: &mut App<'_>) -> Result<()> {
    app.prepare()?;
    let view = gather(app)?;
    if app.json {
        return output::json(app.out, &view);
    }
    print(app, &view)
}

fn gather(app: &App<'_>) -> Result<View> {
    let connection = app.connection();
    let agents: Vec<Agents> = app
        .store
        .session_counts()?
        .into_iter()
        .map(|(agent_harness, sessions)| Agents {
            agent_harness,
            sessions,
        })
        .collect();
    let today = app.today();
    let entries = app.entries(&[Status::Draft, Status::Rejected, Status::Submitted])?;
    let (mut total, mut on_tickets) = (0_i64, 0_i64);
    for entry in entries
        .iter()
        .filter(|entry| day_of(entry.started_ms) == today)
    {
        total = total.saturating_add(entry.value.nanos);
        on_tickets = on_tickets.saturating_add(entry.on_tickets_nanos());
    }
    let (mut drafts, mut sheet_total, mut sheet_on_tickets) = (0_usize, 0_i64, 0_i64);
    for entry in entries
        .iter()
        .filter(|entry| entry.row.status != Status::Submitted)
    {
        drafts = drafts.saturating_add(1);
        sheet_total = sheet_total.saturating_add(entry.value.nanos);
        sheet_on_tickets = sheet_on_tickets.saturating_add(entry.on_tickets_nanos());
    }
    let cadence = app.context.cadence.as_ref();
    // Late: drafts from before this cadence period were never submitted.
    let is_late = cadence.is_some_and(|cadence| {
        let start = cadence
            .period_start
            .parse::<jiff::civil::Date>()
            .map_or(0, start_of_day_ms);
        entries
            .iter()
            .any(|entry| entry.row.status != Status::Submitted && entry.started_ms < start)
    });
    let token_sheet = Sheet {
        token_entries: drafts,
        cost: decimal(sheet_total, CURRENCY),
        share_on_tickets: fraction(sheet_on_tickets, sheet_total),
        period_start: cadence.map(|c| c.period_start.clone()),
        period_end: cadence.map(|c| c.period_end.clone()),
        due_at: cadence.and_then(|c| c.due_at.clone()),
        is_late,
    };
    Ok(View {
        connected: connection.is_some(),
        service: connection.map(|c| c.service),
        organization: app.context.organization.as_ref().map(|o| o.name.clone()),
        person: app.context.person.as_ref().map(|p| p.email.clone()),
        currency: CURRENCY.code(),
        agents,
        rules: app.context.rules.len(),
        today: Today {
            cost: decimal(total, CURRENCY),
            share_on_tickets: fraction(on_tickets, total),
            nanos: total,
            percent: percent(on_tickets, total),
        },
        token_sheet,
    })
}

fn print(app: &mut App<'_>, view: &View) -> Result<()> {
    match (&view.service, &view.organization) {
        (Some(service), Some(name)) => writeln!(app.out, "Organization  {name} ({service})")?,
        (Some(service), None) => writeln!(app.out, "Service       {service}")?,
        (None, _) => writeln!(
            app.out,
            "Organization  none: everything stays on this machine (tc connect URL to connect)"
        )?,
    }
    if let Some(person) = &view.person {
        writeln!(app.out, "Person        {person}")?;
    }
    let found: Vec<String> = view
        .agents
        .iter()
        .map(|a| format!("{} ({} sessions)", a.agent_harness, a.sessions))
        .collect();
    let found = if found.is_empty() {
        "no session files found yet".to_owned()
    } else {
        found.join(", ")
    };
    writeln!(app.out, "Agents        {found}")?;
    writeln!(
        app.out,
        "Rules         {} plus the built-in ones",
        view.rules
    )?;
    writeln!(
        app.out,
        "Today         {} · {}% on tickets",
        display(view.today.nanos, CURRENCY),
        view.today.percent
    )?;
    let sheet = &view.token_sheet;
    writeln!(
        app.out,
        "Drafts        {} to review (tc review)",
        sheet.token_entries
    )?;
    if let (Some(start), Some(end)) = (&sheet.period_start, &sheet.period_end) {
        let note = if sheet.is_late {
            " · late: submit with tc submit"
        } else {
            ""
        };
        writeln!(app.out, "Token sheet   {start} to {end}{note}")?;
    }
    Ok(())
}
