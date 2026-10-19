//! `tc status`: organization, person, agents found, rules, today's cost and
//! share on features, the last submit, and an org manager's request to
//! submit.

use serde::Serialize;
use tokencontroller_core::Money;

use crate::app::App;
use crate::context::AskedToSubmit;
use crate::error::Result;
use crate::log::Status;
use crate::output::{self, decimal, display, percent};

#[derive(Debug, Serialize)]
struct Agents {
    agent_harness: String,
    sessions: u64,
}

#[derive(Debug, Serialize)]
struct Today {
    cost: String,
    currency: String,
    share_on_features_percent: u64,
    #[serde(skip)]
    amount: Money,
}

#[derive(Debug, Serialize)]
struct LastSubmit {
    from: String,
    to: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct View {
    connected: bool,
    service: Option<String>,
    organization: Option<String>,
    person: Option<String>,
    agents: Vec<Agents>,
    rules: usize,
    open: usize,
    today: Today,
    last_submit: Option<LastSubmit>,
    cutoff: Option<String>,
    asked_to_submit: Option<AskedToSubmit>,
    as_of: String,
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
    let state = app.fold()?;
    let sessions = crate::log::sessions(&state, &app.store.sessions()?)?;
    let currency = state.currency();
    let mut total = Money::zero(currency);
    let mut on_features = Money::zero(currency);
    for session in sessions
        .iter()
        .filter(|session| session.started_at.date() == today)
    {
        if let Some(cost) = session.cost {
            total = total.checked_add(cost)?;
        }
        on_features = on_features.checked_add(session.on_features())?;
    }
    let open = sessions
        .iter()
        .filter(|session| session.status != Status::Submitted)
        .count();
    let last_submit = state
        .submissions()
        .iter()
        .max_by_key(|submission| submission.window.to)
        .map(|submission| LastSubmit {
            from: submission.window.from.to_string(),
            to: submission.window.to.to_string(),
            status: submission.status.as_str().to_owned(),
        });
    Ok(View {
        connected: connection.is_some(),
        service: connection.map(|c| c.service),
        organization: app.context.organization.as_ref().map(|o| o.name.clone()),
        person: app.context.person.as_ref().map(|p| p.email.clone()),
        agents,
        rules: state.rules().count(),
        open,
        today: Today {
            cost: decimal(total),
            currency: currency.code(),
            share_on_features_percent: percent(on_features.nanos(), total.nanos()),
            amount: total,
        },
        last_submit,
        cutoff: state.cutoff().map(|date| date.to_string()),
        asked_to_submit: app.context.asked_to_submit.clone(),
        as_of: output::utc(state.as_of()),
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
    writeln!(app.out, "Rules         {}", view.rules)?;
    writeln!(
        app.out,
        "Today         {} · {}% on features",
        display(view.today.amount),
        view.today.share_on_features_percent
    )?;
    writeln!(
        app.out,
        "Open          {} sessions to check (tc review)",
        view.open
    )?;
    match &view.last_submit {
        Some(last) => writeln!(
            app.out,
            "Last submit   {} to {} · {}",
            last.from, last.to, last.status
        )?,
        None => writeln!(app.out, "Last submit   none yet (tc submit)")?,
    }
    if let Some(cutoff) = &view.cutoff {
        writeln!(app.out, "Cutoff        {cutoff}")?;
    }
    if let Some(asked) = &view.asked_to_submit {
        let by = asked
            .asked_by
            .as_deref()
            .map(|name| format!(" by {name}"))
            .unwrap_or_default();
        writeln!(
            app.out,
            "Asked         asked{by} to submit before the cutoff moves to {} (tc submit)",
            asked.cutoff
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::Currency;

    #[test]
    fn the_json_names_today_s_cost_and_the_share_on_features() {
        let view = View {
            connected: false,
            service: None,
            organization: None,
            person: None,
            agents: vec![Agents {
                agent_harness: "claude-code".into(),
                sessions: 3,
            }],
            rules: 2,
            open: 3,
            today: Today {
                cost: "4.50".into(),
                currency: "USD".into(),
                share_on_features_percent: 80,
                amount: Money::from_nanos(4_500_000_000, Currency::USD),
            },
            last_submit: Some(LastSubmit {
                from: "2026-10-01".into(),
                to: "2026-10-11".into(),
                status: "approved".into(),
            }),
            cutoff: None,
            asked_to_submit: None,
            as_of: "2026-10-19T09:00:00Z".into(),
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(
            json["today"],
            serde_json::json!({"cost": "4.50", "currency": "USD", "share_on_features_percent": 80})
        );
        assert_eq!(json["asked_to_submit"], serde_json::Value::Null);
        assert_eq!(json["last_submit"]["status"], "approved");
        assert_eq!(json["agents"][0]["sessions"], 3);
        assert_eq!(json["open"], 3);
    }
}
