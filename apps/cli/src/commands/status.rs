//! `tc status`: organization, person, agents found, rules, and today's cost
//! and share on tickets.

use serde::Serialize;

use crate::amounts::{decimal, display};
use crate::app::{App, day_of, start_of_day_ms};
use crate::context::AskedToSubmit;
use crate::entries::CURRENCY;
use crate::error::Result;
use crate::output::{self, percent};
use crate::store::Status;

#[derive(Debug, Serialize)]
struct Agents {
    agent_harness: String,
    sessions: u64,
}

#[derive(Debug, Serialize)]
struct Today {
    list_value: String,
    currency: String,
    share_on_tickets_percent: u64,
    #[serde(skip)]
    nanos: i64,
}

#[derive(Debug, Serialize)]
struct Sheet {
    period_start: String,
    period_end: String,
    late: bool,
}

#[derive(Debug, Serialize)]
struct View {
    connected: bool,
    service: Option<String>,
    organization: Option<String>,
    person: Option<String>,
    agents: Vec<Agents>,
    rules: usize,
    drafts: usize,
    today: Today,
    token_sheet: Option<Sheet>,
    asked_to_submit: Option<AskedToSubmit>,
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
    let drafts = entries
        .iter()
        .filter(|entry| entry.row.status != Status::Submitted)
        .count();
    let token_sheet = app.context.cadence.as_ref().map(|cadence| {
        let start = cadence
            .period_start
            .parse::<jiff::civil::Date>()
            .map_or(0, start_of_day_ms);
        // Late: the service says an earlier period waits, or drafts from
        // before this cadence period were never submitted.
        let late = cadence.late
            || entries
                .iter()
                .any(|entry| entry.row.status != Status::Submitted && entry.started_ms < start);
        Sheet {
            period_start: cadence.period_start.clone(),
            period_end: cadence.period_end.clone(),
            late,
        }
    });
    Ok(View {
        connected: connection.is_some(),
        service: connection.map(|c| c.service),
        organization: app.context.organization.as_ref().map(|o| o.name.clone()),
        person: app.context.person.as_ref().map(|p| p.email.clone()),
        agents,
        rules: own_rules(app),
        drafts,
        today: Today {
            list_value: decimal(total, CURRENCY),
            currency: CURRENCY.code(),
            share_on_tickets_percent: percent(on_tickets, total),
            nanos: total,
        },
        token_sheet,
        asked_to_submit: app.context.asked_to_submit.clone(),
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
        view.today.share_on_tickets_percent
    )?;
    writeln!(
        app.out,
        "Drafts        {} to review (tc review)",
        view.drafts
    )?;
    if let Some(sheet) = &view.token_sheet {
        let note = if sheet.late {
            " · late: submit with tc submit"
        } else {
            ""
        };
        writeln!(
            app.out,
            "Token sheet   {} to {}{note}",
            sheet.period_start, sheet.period_end
        )?;
    }
    if let Some(asked) = &view.asked_to_submit {
        let by = asked
            .asked_by
            .as_deref()
            .map(|name| format!(" by {name}"))
            .unwrap_or_default();
        writeln!(
            app.out,
            "Close         asked{by} to submit for {} (tc submit)",
            asked.period
        )?;
    }
    Ok(())
}

/// The organization's rules besides the built-in ones, and the person's.
fn own_rules(app: &App<'_>) -> usize {
    let built_in = tokencontroller_core::Rule::defaults();
    app.context
        .organization_rules()
        .iter()
        .filter(|rule| !built_in.contains(rule))
        .count()
        .saturating_add(app.context.rules.len())
}
