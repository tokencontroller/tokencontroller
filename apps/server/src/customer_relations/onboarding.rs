//! Onboarding emails (customer relations, loop 5). A new organization takes
//! four first steps: its first token entry, first token sheet, first bill
//! and first close. When it has not taken the next one three days after its
//! last progress, its controllers get one short email about that step.
//!
//! One email per organization and step, ever. None after the first close or
//! after 30 days without activity. A token sheet is only needed where people
//! run agents on their own machines, so after its email the next step counts.
//! Off unless `TC_ONBOARDING_EMAILS` is `on`; then the job runner checks
//! every hour as the job `onboarding-emails`.

use std::collections::HashMap;

use lettre::message::Mailbox;
use serde::Serialize;
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use super::mail::{self, Outgoing};
use crate::AppState;

/// How long an organization may sit at a step before it gets the email.
const STALLED_AFTER: Duration = Duration::days(3);

/// After this long without any activity, no more emails.
const QUIET_FOR: Duration = Duration::days(30);

/// The docs the emails link to.
const DOCS: &str = "https://docs.tokencontroller.com";

/// The job's name in the job runner.
pub const JOB: &str = "onboarding-emails";

/// How often the job runner checks for due emails.
pub const JOB_EVERY: std::time::Duration = std::time::Duration::from_hours(1);

/// One of the four first steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    TokenEntry,
    TokenSheet,
    Bill,
    Close,
}

impl Step {
    const ALL: [Self; 4] = [Self::TokenEntry, Self::TokenSheet, Self::Bill, Self::Close];

    const fn as_str(self) -> &'static str {
        match self {
            Self::TokenEntry => "token_entry",
            Self::TokenSheet => "token_sheet",
            Self::Bill => "bill",
            Self::Close => "close",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.as_str() == value)
    }

    /// The subject and the part of the email that is about this step.
    fn email(self, organization: &str) -> (&'static str, String) {
        match self {
            Self::TokenEntry => (
                "Your first token entries",
                format!(
                    "{organization} has no token entries yet. A token entry is one agent session's tokens, cost and work, and every report starts from them.\n\n\
                     The fastest start is telemetry push: an administrator sets it once in Claude Code's managed settings, and nobody installs anything.\n\
                     {DOCS}/collect/telemetry\n\n\
                     Every door, by agent: {DOCS}/collect"
                ),
            ),
            Self::TokenSheet => (
                "Your first token sheet",
                format!(
                    "{organization} has token entries, but nobody has submitted a token sheet yet. On a token sheet an agent manager confirms which ticket each of their sessions was for and what it did. Telemetry carries the cost, never the ticket.\n\n\
                     Agent managers submit one from Claude Code with the mod, or with the CLI:\n\
                     {DOCS}/collect/claude-code\n\
                     {DOCS}/collect/cli\n\n\
                     If your agents only run in CI or in the cloud, there is nothing to do here."
                ),
            ),
            Self::Bill => (
                "Your first bill",
                format!(
                    "{organization} has no bill yet. A bill is your provider's figure for a month's metered usage, and your token entries must add up to it.\n\n\
                     Connect the Anthropic or OpenAI API once, or upload the provider's CSV:\n\
                     {DOCS}/account/bills"
                ),
            ),
            Self::Close => (
                "Your first close",
                format!(
                    "{organization} has not closed a period yet. The close locks a month, so its token entries can no longer change, and produces the cost sheet.\n\n\
                     {DOCS}/account/close"
                ),
            ),
        }
    }
}

/// An onboarding email an organization's controllers got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SentEmail {
    pub step: Step,
    #[serde(with = "time::serde::rfc3339")]
    pub sent_at: OffsetDateTime,
}

/// How far an organization got with its first steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Journey {
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_token_entry: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_token_sheet: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_bill: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub first_close: Option<OffsetDateTime>,
    /// Oldest first.
    pub emails: Vec<SentEmail>,
}

/// One organization's first steps, as the emails need them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Progress {
    organization_id: Uuid,
    name: String,
    signed_up: OffsetDateTime,
    journey: Journey,
    /// When its earliest token entry started: the first close can come only
    /// after that month ended.
    first_usage: Option<OffsetDateTime>,
    last_activity: OffsetDateTime,
}

impl Progress {
    fn done(&self, step: Step) -> Option<OffsetDateTime> {
        match step {
            Step::TokenEntry => self.journey.first_token_entry,
            Step::TokenSheet => self.journey.first_token_sheet,
            Step::Bill => self.journey.first_bill,
            Step::Close => self.journey.first_close,
        }
    }

    fn emailed(&self, step: Step) -> bool {
        self.journey.emails.iter().any(|email| email.step == step)
    }

    /// The step whose email is due at `now`, if any.
    fn due(&self, now: OffsetDateTime) -> Option<Step> {
        let quiet = self
            .last_activity
            .checked_add(QUIET_FOR)
            .is_none_or(|until| now > until);
        if self.journey.first_close.is_some() || quiet {
            return None;
        }
        let next = Step::ALL.into_iter().find(|step| {
            self.done(*step).is_none() && !(*step == Step::TokenSheet && self.emailed(*step))
        })?;
        if self.emailed(next) {
            return None;
        }
        let mut since = Step::ALL
            .into_iter()
            .filter_map(|step| self.done(step))
            .chain(self.journey.emails.iter().map(|email| email.sent_at))
            .fold(self.signed_up, OffsetDateTime::max);
        if next == Step::Close {
            since = since.max(month_end(self.first_usage?)?);
        }
        let stalled = since.checked_add(STALLED_AFTER).is_some_and(|at| now >= at);
        stalled.then_some(next)
    }
}

/// The first moment after the calendar month of `moment`, in UTC.
fn month_end(moment: OffsetDateTime) -> Option<OffsetDateTime> {
    let date = moment.to_offset(time::UtcOffset::UTC).date();
    let year = if date.month() == time::Month::December {
        date.year().checked_add(1)?
    } else {
        date.year()
    };
    let first = Date::from_calendar_date(year, date.month().next(), 1).ok()?;
    Some(first.midnight().assume_utc())
}

/// The onboarding emails sent, by organization, oldest first.
async fn sent(
    db: &PgPool,
    only: Option<Uuid>,
) -> Result<HashMap<Uuid, Vec<SentEmail>>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT organization_id, step, sent_at FROM onboarding_emails
         WHERE $1::uuid IS NULL OR organization_id = $1
         ORDER BY sent_at, step",
        only
    )
    .fetch_all(db)
    .await?;
    let mut sent: HashMap<Uuid, Vec<SentEmail>> = HashMap::new();
    for row in rows {
        if let Some(step) = Step::parse(&row.step) {
            sent.entry(row.organization_id)
                .or_default()
                .push(SentEmail {
                    step,
                    sent_at: row.sent_at,
                });
        }
    }
    Ok(sent)
}

/// The first steps of one organization, or of every organization that has
/// not closed a period yet.
async fn progress(db: &PgPool, only: Option<Uuid>) -> Result<Vec<Progress>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT o.id, o.name, o.created_at,
             e.first_entry, e.last_entry, e.first_usage,
             s.first_sheet, s.last_sheet,
             b.first_bill, b.last_bill,
             c.first_close, c.last_close
           FROM organizations o
           CROSS JOIN LATERAL (
             SELECT min(t.created_at) AS first_entry, max(t.created_at) AS last_entry,
                    min(t.started_at) AS first_usage
             FROM token_entries t WHERE t.organization_id = o.id) e
           CROSS JOIN LATERAL (
             SELECT min(t.first_submitted_at) AS first_sheet, max(t.last_submitted_at) AS last_sheet
             FROM token_sheets t WHERE t.organization_id = o.id) s
           CROSS JOIN LATERAL (
             SELECT min(t.created_at) AS first_bill, max(t.created_at) AS last_bill
             FROM bills t WHERE t.organization_id = o.id) b
           CROSS JOIN LATERAL (
             SELECT min(t.closed_at) AS first_close, max(t.closed_at) AS last_close
             FROM closed_periods t WHERE t.organization_id = o.id) c
           WHERE CASE WHEN $1::uuid IS NULL
                   THEN NOT EXISTS (SELECT 1 FROM closed_periods t WHERE t.organization_id = o.id)
                   ELSE o.id = $1 END
           ORDER BY o.created_at, o.id"#,
        only
    )
    .fetch_all(db)
    .await?;
    let mut sent = sent(db, only).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let last_activity = [
                row.last_entry,
                row.last_sheet,
                row.last_bill,
                row.last_close,
            ]
            .into_iter()
            .flatten()
            .fold(row.created_at, OffsetDateTime::max);
            Progress {
                organization_id: row.id,
                name: row.name,
                signed_up: row.created_at,
                journey: Journey {
                    first_token_entry: row.first_entry,
                    first_token_sheet: row.first_sheet,
                    first_bill: row.first_bill,
                    first_close: row.first_close,
                    emails: sent.remove(&row.id).unwrap_or_default(),
                },
                first_usage: row.first_usage,
                last_activity,
            }
        })
        .collect())
}

/// How far one organization got with its first steps, for its customer
/// record.
///
/// # Errors
///
/// When the database fails.
pub async fn journey(db: &PgPool, organization_id: Uuid) -> Result<Option<Journey>, sqlx::Error> {
    Ok(progress(db, Some(organization_id))
        .await?
        .into_iter()
        .next()
        .map(|progress| progress.journey))
}

/// Sends the onboarding emails that are due at `now`, and says to how many
/// organizations. Does nothing unless `TC_ONBOARDING_EMAILS` is `on` and a
/// mail server is set.
///
/// # Errors
///
/// When the database fails. A mail that cannot be sent is logged, and its
/// step is tried again on the next run if no controller got it.
pub async fn send_due(state: &AppState, now: OffsetDateTime) -> anyhow::Result<usize> {
    let Some(smtp_url) = state.config.smtp_url.as_deref() else {
        return Ok(0);
    };
    if !state.config.customer_relations.onboarding_emails {
        return Ok(0);
    }
    let mut sent = 0_usize;
    for organization in progress(&state.db, None).await? {
        let Some(step) = organization.due(now) else {
            continue;
        };
        let claimed = sqlx::query_scalar!(
            r#"INSERT INTO onboarding_emails (organization_id, step) VALUES ($1, $2)
               ON CONFLICT DO NOTHING RETURNING true AS "claimed!""#,
            organization.organization_id,
            step.as_str()
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();
        if !claimed {
            continue;
        }
        if send(state, smtp_url, &organization, step).await? {
            sent = sent.saturating_add(1);
        } else {
            sqlx::query!(
                "DELETE FROM onboarding_emails WHERE organization_id = $1 AND step = $2",
                organization.organization_id,
                step.as_str()
            )
            .execute(&state.db)
            .await?;
        }
    }
    Ok(sent)
}

/// The job runner's entry: sends what is due now.
///
/// # Errors
///
/// Fails as [`send_due`] does.
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let sent = send_due(&state, OffsetDateTime::now_utc()).await?;
    Ok(format!("sent onboarding emails to {sent} organizations"))
}

/// Sends `step`'s email to the organization's controllers; whether at least
/// one of them got it.
async fn send(
    state: &AppState,
    smtp_url: &str,
    organization: &Progress,
    step: Step,
) -> Result<bool, sqlx::Error> {
    let controllers = sqlx::query!(
        "SELECT email, name FROM people
         WHERE organization_id = $1 AND role = 'controller'
           AND joined_at IS NOT NULL AND removed_at IS NULL
         ORDER BY joined_at, id",
        organization.organization_id
    )
    .fetch_all(&state.db)
    .await?;
    let support_from = &state.config.customer_relations.support_from;
    let questions = if support_from.is_some() {
        " Questions? Reply to this email."
    } else {
        ""
    };
    let from = crate::mail::sender(&state.config);
    let (subject, about) = step.email(&organization.name);
    let mut delivered = false;
    for controller in controllers {
        let name = controller.name.trim();
        let greeting = if name.is_empty() {
            "Hello,".to_owned()
        } else {
            format!("Hello {name},")
        };
        let result = match controller.email.parse::<Mailbox>() {
            Ok(to) => {
                let outgoing = Outgoing {
                    from: from.clone(),
                    to,
                    reply_to: support_from.clone(),
                    subject: subject.to_owned(),
                    text: format!(
                        "{greeting}\n\n{about}\n\nWe send one short note like this for each first step, and none after your first close.{questions}\n\n--\nToken Controller"
                    ),
                    message_id: None,
                    references: Vec::new(),
                    headers: Vec::new(),
                };
                mail::send_smtp(smtp_url, &outgoing).await
            }
            Err(error) => Err(error.into()),
        };
        match result {
            Ok(()) => delivered = true,
            Err(error) => tracing::warn!(
                organization = %organization.organization_id,
                step = step.as_str(),
                "could not send an onboarding email: {error:#}"
            ),
        }
    }
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const SIGNED_UP: OffsetDateTime = datetime!(2026-10-12 9:00 UTC);

    fn new_organization() -> Progress {
        Progress {
            organization_id: Uuid::nil(),
            name: "Kestrel Digital".to_owned(),
            signed_up: SIGNED_UP,
            journey: Journey {
                first_token_entry: None,
                first_token_sheet: None,
                first_bill: None,
                first_close: None,
                emails: Vec::new(),
            },
            first_usage: None,
            last_activity: SIGNED_UP,
        }
    }

    fn emailed(progress: &mut Progress, step: Step, sent_at: OffsetDateTime) {
        progress.journey.emails.push(SentEmail { step, sent_at });
    }

    #[test]
    fn a_stalled_step_gets_one_email_after_three_days() {
        let mut progress = new_organization();
        assert_eq!(progress.due(SIGNED_UP + Duration::days(2)), None);
        assert_eq!(
            progress.due(SIGNED_UP + Duration::days(3)),
            Some(Step::TokenEntry)
        );
        emailed(
            &mut progress,
            Step::TokenEntry,
            SIGNED_UP + Duration::days(3),
        );
        assert_eq!(progress.due(SIGNED_UP + Duration::days(20)), None);
    }

    #[test]
    fn progress_restarts_the_wait() {
        let mut progress = new_organization();
        let entry = SIGNED_UP + Duration::days(5);
        progress.journey.first_token_entry = Some(entry);
        progress.first_usage = Some(entry);
        progress.last_activity = entry;
        assert_eq!(progress.due(entry + Duration::days(2)), None);
        assert_eq!(
            progress.due(entry + Duration::days(3)),
            Some(Step::TokenSheet)
        );
    }

    #[test]
    fn after_the_token_sheet_email_the_bill_counts() {
        let mut progress = new_organization();
        let entry = SIGNED_UP + Duration::hours(1);
        progress.journey.first_token_entry = Some(entry);
        progress.first_usage = Some(entry);
        progress.last_activity = SIGNED_UP + Duration::days(9);
        let sheet_email = SIGNED_UP + Duration::days(3);
        emailed(&mut progress, Step::TokenSheet, sheet_email);
        assert_eq!(progress.due(sheet_email + Duration::days(2)), None);
        assert_eq!(
            progress.due(sheet_email + Duration::days(3)),
            Some(Step::Bill)
        );
    }

    #[test]
    fn the_close_waits_for_the_first_month_to_end() {
        let mut progress = new_organization();
        let entry = SIGNED_UP + Duration::hours(1);
        progress.journey.first_token_entry = Some(entry);
        progress.journey.first_token_sheet = Some(entry);
        progress.journey.first_bill = Some(SIGNED_UP + Duration::days(1));
        progress.first_usage = Some(entry);
        progress.last_activity = datetime!(2026-10-30 9:00 UTC);
        assert_eq!(progress.due(datetime!(2026-11-03 23:59 UTC)), None);
        assert_eq!(
            progress.due(datetime!(2026-11-04 0:00 UTC)),
            Some(Step::Close)
        );
    }

    #[test]
    fn nothing_after_the_close_or_a_quiet_month() {
        let mut progress = new_organization();
        assert_eq!(progress.due(SIGNED_UP + Duration::days(31)), None);
        progress.journey.first_close = Some(SIGNED_UP);
        assert_eq!(progress.due(SIGNED_UP + Duration::days(4)), None);
    }

    #[test]
    fn december_ends_in_the_next_year() {
        assert_eq!(
            month_end(datetime!(2026-12-31 23:00 UTC)),
            Some(datetime!(2027-01-01 0:00 UTC))
        );
        assert_eq!(
            month_end(datetime!(2026-10-12 9:00 +02:00)),
            Some(datetime!(2026-11-01 0:00 UTC))
        );
    }

    #[test]
    fn every_step_names_the_organization_and_links_the_docs() {
        for step in Step::ALL {
            let (subject, text) = step.email("Kestrel Digital");
            assert!(subject.starts_with("Your first"));
            assert!(text.starts_with("Kestrel Digital has"));
            assert!(text.contains(DOCS));
            assert_eq!(Step::parse(step.as_str()), Some(step));
        }
    }
}
