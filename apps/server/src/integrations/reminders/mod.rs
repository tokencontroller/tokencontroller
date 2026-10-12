//! Late token sheet reminders: one mail to the agent manager when a token
//! sheet's cadence period is over and it is not submitted. Nothing is
//! blocked and nothing is capped (docs page Cadence and token sheets). The
//! web app and the Claude Code mod show late token sheets on their own.

use std::collections::BTreeMap;
use std::time::Duration;

use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::AppState;
use crate::mail::{self, Delivery, Mail};

const TEXT: &str = include_str!("late-token-sheet.txt");
const HTML: &str = include_str!("late-token-sheet.html");

/// The subject of every late token sheet reminder.
pub const SUBJECT: &str = "Your token sheet is late";

/// The job that sends late token sheet reminders.
pub const JOB: &str = "late-reminders";

/// How often the job looks for late token sheets.
pub const JOB_EVERY: Duration = Duration::from_hours(1);

/// Days after its cadence period ends that a late token sheet still gets a
/// mail. Older ones show in the web app and the Claude Code mod only, so a
/// new cadence or a new instance does not mail about long-gone weeks.
const RECENT_DAYS: i64 = 14;

/// Mails one run sends at most; the rest wait for the next run.
const PER_RUN: usize = 200;

/// A token sheet whose cadence period is over and that is not submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateSheet {
    /// The agent manager's email address.
    pub email: String,
    /// The first day of the cadence period.
    pub period_start: Date,
    /// The last day of the cadence period.
    pub period_end: Date,
    /// How many token entries wait on it.
    pub token_entries: u64,
}

/// A reminder ready to send, with a plain-text and an HTML part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub to: String,
    pub subject: &'static str,
    pub text: String,
    pub html: String,
}

/// A cadence period in words, such as `5 to 11 October 2026`, or
/// `28 September to 4 October 2026` across a month.
#[must_use]
pub fn period_words(start: Date, end: Date) -> String {
    let end_words = format!("{} {} {}", end.day(), end.month(), end.year());
    if start == end {
        end_words
    } else if start.year() != end.year() {
        format!(
            "{} {} {} to {end_words}",
            start.day(),
            start.month(),
            start.year()
        )
    } else if start.month() == end.month() {
        format!("{} to {end_words}", start.day())
    } else {
        format!("{} {} to {end_words}", start.day(), start.month())
    }
}

/// What waits on the token sheet, in one sentence.
fn waiting_words(token_entries: u64) -> String {
    match token_entries {
        0 => "If there was nothing to report, submit it empty.".to_owned(),
        1 => "It holds 1 token entry waiting for your review.".to_owned(),
        many => format!("It holds {many} token entries waiting for your review."),
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn fill(template: &str, values: &[(&str, String)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{{{name}}}}}"), value)
        })
}

/// The reminder for one late token sheet. `public_url` is `TC_PUBLIC_URL`;
/// the link opens the person's token sheet in the web app.
#[must_use]
pub fn late_sheet_reminder(sheet: &LateSheet, public_url: &str) -> Reminder {
    let public_url = public_url.trim_end_matches('/');
    let url = format!("{public_url}/review");
    let host = public_url
        .split_once("://")
        .map_or(public_url, |(_, rest)| rest)
        .to_owned();
    let values = [
        ("period", period_words(sheet.period_start, sheet.period_end)),
        ("waiting", waiting_words(sheet.token_entries)),
        ("token_sheet_url", url),
        ("host", host),
    ];
    let escaped: Vec<(&str, String)> = values
        .iter()
        .map(|(name, value)| (*name, escape_html(value)))
        .collect();
    Reminder {
        to: sheet.email.clone(),
        subject: SUBJECT,
        text: fill(TEXT, &values),
        html: fill(HTML, &escaped),
    }
}

/// The `late-reminders` job.
///
/// # Errors
///
/// As [`send_due`].
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    send_due(&state, OffsetDateTime::now_utc().date()).await
}

/// One late token sheet the job may mail about.
struct Due {
    organization_id: Uuid,
    person_id: Uuid,
    sheet: LateSheet,
}

/// The newest late token sheet of each person whose cadence period ended
/// in the last [`RECENT_DAYS`] days.
async fn recent_late_sheets(db: &sqlx::PgPool, today: Date) -> Result<Vec<Due>, sqlx::Error> {
    let oldest = today
        .checked_sub(time::Duration::days(RECENT_DAYS))
        .unwrap_or(Date::MIN);
    let organizations =
        sqlx::query_scalar!("SELECT DISTINCT organization_id FROM token_sheets ORDER BY 1")
            .fetch_all(db)
            .await?;
    let mut due = Vec::new();
    for organization_id in organizations {
        let late = crate::token_sheets::late::late_sheets(db, organization_id, None, today).await?;
        // Oldest first, so each person keeps their newest.
        let newest: BTreeMap<Uuid, _> = late
            .into_iter()
            .filter(|sheet| sheet.period_end >= oldest)
            .map(|sheet| (sheet.person_id, sheet))
            .collect();
        for (person_id, sheet) in newest {
            let token_entries = drafts(
                db,
                organization_id,
                person_id,
                sheet.period_start,
                sheet.period_end,
            )
            .await?;
            due.push(Due {
                organization_id,
                person_id,
                sheet: LateSheet {
                    email: sheet.email,
                    period_start: sheet.period_start,
                    period_end: sheet.period_end,
                    token_entries,
                },
            });
        }
    }
    Ok(due)
}

/// How many draft token entries of a person started in a cadence period.
async fn drafts(
    db: &sqlx::PgPool,
    organization_id: Uuid,
    person_id: Uuid,
    first_day: Date,
    last_day: Date,
) -> Result<u64, sqlx::Error> {
    let from = first_day.midnight().assume_utc();
    let until = last_day
        .next_day()
        .unwrap_or(last_day)
        .midnight()
        .assume_utc();
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM token_entries
           WHERE organization_id = $1 AND agent_manager_id = $2 AND status = 'draft'
             AND started_at >= $3 AND started_at < $4"#,
        organization_id,
        person_id,
        from,
        until
    )
    .fetch_one(db)
    .await?;
    Ok(u64::try_from(count).unwrap_or(0))
}

/// Mails the agent manager of each late token sheet once: the newest late
/// token sheet of each person, if its cadence period ended in the last
/// [`RECENT_DAYS`] days, at most [`PER_RUN`] mails a run. Without a mail
/// server it sends nothing and records nothing.
///
/// # Errors
///
/// Fails when the database fails, or when the mail server refused every
/// reminder it tried, so the status page shows the outage.
pub async fn send_due(state: &AppState, today: Date) -> anyhow::Result<String> {
    if state.config.smtp_url.is_none() {
        return Ok("no mail server is set (TC_SMTP_URL), so no reminders were sent".to_owned());
    }
    let mut sent = 0_usize;
    let mut tried = 0_usize;
    let mut failures = Vec::new();
    for due in recent_late_sheets(&state.db, today).await? {
        if tried >= PER_RUN {
            break;
        }
        // Claim the token sheet first, so it is never mailed twice.
        let claimed = sqlx::query_scalar!(
            "INSERT INTO late_sheet_reminders (organization_id, person_id, starts_on)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING RETURNING person_id",
            due.organization_id,
            due.person_id,
            due.sheet.period_start
        )
        .fetch_optional(&state.db)
        .await?;
        if claimed.is_none() {
            continue;
        }
        tried = tried.saturating_add(1);
        let reminder = late_sheet_reminder(&due.sheet, &state.config.public_url);
        let mail = Mail {
            kind: "late_token_sheet",
            to: reminder.to,
            subject: reminder.subject.to_owned(),
            text: reminder.text,
            html: reminder.html,
        };
        match mail::send(&state.db, &state.config, mail).await {
            Ok(Delivery::Sent) => sent = sent.saturating_add(1),
            result => {
                // Not sent: give it back, so the next run tries again.
                sqlx::query!(
                    "DELETE FROM late_sheet_reminders
                     WHERE organization_id = $1 AND person_id = $2 AND starts_on = $3",
                    due.organization_id,
                    due.person_id,
                    due.sheet.period_start
                )
                .execute(&state.db)
                .await?;
                if let Err(error) = result {
                    failures.push(format!("{error:#}"));
                }
            }
        }
    }
    if tried > 0 && failures.len() == tried {
        anyhow::bail!(
            "the mail server took none of {tried} late token sheet reminders: {}",
            failures.first().map_or("", String::as_str)
        );
    }
    Ok(format!("{sent} of {tried} late token sheet reminders sent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn periods_read_as_words() {
        assert_eq!(
            period_words(date!(2026 - 10 - 05), date!(2026 - 10 - 11)),
            "5 to 11 October 2026"
        );
        assert_eq!(
            period_words(date!(2026 - 09 - 28), date!(2026 - 10 - 04)),
            "28 September to 4 October 2026"
        );
        assert_eq!(
            period_words(date!(2026 - 12 - 28), date!(2027 - 01 - 03)),
            "28 December 2026 to 3 January 2027"
        );
        assert_eq!(
            period_words(date!(2026 - 10 - 05), date!(2026 - 10 - 05)),
            "5 October 2026"
        );
    }

    #[test]
    fn the_reminder_fills_both_parts() {
        let reminder = late_sheet_reminder(
            &LateSheet {
                email: "m.ortner@kestrel.example".to_owned(),
                period_start: date!(2026 - 10 - 05),
                period_end: date!(2026 - 10 - 11),
                token_entries: 14,
            },
            "https://app.tokencontroller.com/",
        );
        assert_eq!(reminder.to, "m.ortner@kestrel.example");
        assert_eq!(reminder.subject, "Your token sheet is late");
        for part in [&reminder.text, &reminder.html] {
            assert!(part.contains("5 to 11 October 2026"));
            assert!(part.contains("It holds 14 token entries waiting for your review."));
            assert!(part.contains("https://app.tokencontroller.com/review"));
            assert!(part.contains("Token Controller · app.tokencontroller.com"));
            assert!(!part.contains("{{"));
        }
    }

    #[test]
    fn what_waits_reads_as_a_sentence() {
        assert_eq!(
            waiting_words(0),
            "If there was nothing to report, submit it empty."
        );
        assert_eq!(
            waiting_words(1),
            "It holds 1 token entry waiting for your review."
        );
        assert_eq!(
            waiting_words(2),
            "It holds 2 token entries waiting for your review."
        );
    }

    #[test]
    fn html_values_are_escaped() {
        assert_eq!(
            escape_html("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
