//! What this machine knows about the person's organization: its name and
//! currency, the tag values and attributes, the organization's rules and the
//! person's own, the price table, the cutoff and the submissions. Connected,
//! it comes from the service (`GET /api/v1/me/context`); local, there is
//! none, and only the rules and splits on this machine apply.
//!
//! Rules, tag values and attributes are the core's own records, so the
//! machine and the service fold the same log the same way (ADR 0010).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::{
    Approval, AttributeValue, Currency, Decision, Event, Money, PersonAttribute, PriceRow,
    PriceSource, Record, Rule, Submit, TagValueRecord, TokenKind,
};

use crate::error::{Error, Result};

/// The person in the log on this machine. The service knows who the person
/// is from the token, so the name never leaves the machine.
pub const PERSON: &str = "me";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    #[serde(default)]
    pub organization: Option<Organization>,
    #[serde(default)]
    pub person: Option<Person>,
    /// The tag names the organization uses besides the built-in ones.
    #[serde(default)]
    pub tag_names: Vec<String>,
    /// Every tag value on record, with its defaults.
    #[serde(default)]
    pub tag_values: Vec<TagValueRecord>,
    #[serde(default)]
    pub attribute_values: Vec<AttributeValue>,
    /// The person's own attributes, such as their team.
    #[serde(default)]
    pub attributes: Vec<PersonAttribute>,
    /// The organization's rules.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The person's own rules, in order. They run after the organization's.
    #[serde(default)]
    pub my_rules: Vec<Rule>,
    /// The price table in force, in the organization's currency.
    #[serde(default)]
    pub price_table: Vec<PriceLine>,
    /// The organization's cutoff date, such as `2026-09-30`.
    #[serde(default)]
    pub cutoff: Option<String>,
    /// The person's submissions.
    #[serde(default)]
    pub submissions: Vec<SubmissionLine>,
    /// An org manager asked everyone to submit before the cutoff moves.
    #[serde(default)]
    pub asked_to_submit: Option<AskedToSubmit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Organization {
    pub name: String,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Person {
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// One row of the price table as the service writes it: the price as a
/// decimal string and the date as `2026-01-01`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PriceLine {
    pub provider: String,
    pub model: String,
    pub kind: TokenKind,
    pub per_million: String,
    pub valid_from: String,
    #[serde(default)]
    pub source: Option<PriceSource>,
}

/// A submission as the service lists it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubmissionLine {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AskedToSubmit {
    /// The date the cutoff moves to, such as `2026-09-30`.
    pub cutoff: String,
    #[serde(default)]
    pub asked_by: Option<String>,
}

impl Context {
    /// The organization's currency, or US dollars with none.
    pub fn currency(&self) -> Currency {
        self.organization
            .as_ref()
            .and_then(|organization| organization.currency.as_deref())
            .and_then(|code| Currency::new(code).ok())
            .unwrap_or(Currency::USD)
    }

    /// Whether a tag name may be set: built-in names always, others only if
    /// the organization lists them. With no organization, any name the core
    /// takes.
    pub fn allows_tag(&self, name: &str) -> bool {
        let Ok(name) = tokencontroller_core::TagName::new(name) else {
            return false;
        };
        name.is_built_in()
            || self.organization.is_none()
            || self
                .tag_names
                .iter()
                .any(|allowed| allowed == name.as_str())
    }

    /// The price table as events, in `currency`.
    pub fn price_events(&self, currency: Currency) -> Result<Vec<Event>> {
        let mut events = Vec::new();
        for (index, line) in self.price_table.iter().enumerate() {
            let per_million = Money::parse(&line.per_million, currency)?;
            let valid_from = crate::output::parse_date(&line.valid_from).ok_or_else(|| {
                Error::Data(format!(
                    "the price table row for {} has no date: {}",
                    line.model, line.valid_from
                ))
            })?;
            let row = PriceRow {
                provider: line.provider.clone(),
                model: line.model.clone(),
                kind: line.kind,
                per_million,
                valid_from,
                source: line.source.unwrap_or(PriceSource::Managed),
            };
            events.push(Event::new(
                &format!("price-{index}"),
                OffsetDateTime::UNIX_EPOCH,
                Record::PriceRow(row),
            )?);
        }
        Ok(events)
    }

    /// The person's submissions as events: a submit each, and the decision
    /// where one was made.
    pub fn submission_events(&self) -> Result<Vec<Event>> {
        let person = tokencontroller_core::TagValue::new(PERSON)?;
        let mut events = Vec::new();
        for line in &self.submissions {
            let (Some(from), Some(to)) = (
                crate::output::parse_date(&line.from),
                crate::output::parse_date(&line.to),
            ) else {
                continue;
            };
            let Ok(window) = tokencontroller_core::DateRange::new(from, to) else {
                continue;
            };
            // Recorded at the start of its window, so the fold sees it
            // whatever day it is; a later submit by hand comes after it.
            let at = from.midnight().assume_utc();
            events.push(Event::new(
                &format!("submission-{}", line.id),
                at,
                Record::Submit(Submit {
                    person: person.clone(),
                    window,
                }),
            )?);
            let decision = match line.status.as_deref() {
                Some("approved") => Some(Decision::Approved),
                Some("rejected") => Some(Decision::Rejected {
                    reason: line.reason.clone().unwrap_or_default(),
                }),
                _ => None,
            };
            if let Some(decision) = decision {
                events.push(Event::new(
                    &format!("approval-{}", line.id),
                    at.saturating_add(time::Duration::seconds(1)),
                    Record::Approval(Approval {
                        submission: tokencontroller_core::EventId::new(&format!(
                            "submission-{}",
                            line.id
                        ))?,
                        decision,
                    }),
                )?);
            }
        }
        Ok(events)
    }
}

impl From<tokencontroller_core::EventIdError> for Error {
    fn from(error: tokencontroller_core::EventIdError) -> Self {
        Self::Data(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::State;

    fn service_context() -> Context {
        let json = r#"{
            "organization": {"name": "Kestrel Digital", "currency": "EUR"},
            "person": {"email": "m.ortner@kestrel.example"},
            "tag_names": ["team", "client"],
            "tag_values": [{"name": "feature", "key": "KD-1", "defaults": {"project": "storefront"}}],
            "attribute_values": [{"name": "team", "key": "web", "defaults": {"department": "engineering"}}],
            "attributes": [{"person": "me", "name": "team", "key": "web"}],
            "rules": [{"name": "key in branch", "scope": "organization", "order": 0, "signal": "branch", "pattern": "(?P<key>KD-\\d+)", "then": {"key_of": {"name": "feature"}}}],
            "my_rules": [{"name": "my spikes", "scope": {"person": "me"}, "order": 0, "signal": "branch", "pattern": "spike", "then": {"tag": {"name": "purpose", "key": "learning"}}}],
            "price_table": [{"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "input", "per_million": "3", "valid_from": "2026-01-01"}],
            "cutoff": "2026-09-30",
            "submissions": [{"id": "sub-1", "from": "2026-10-01", "to": "2026-10-05", "status": "rejected", "reason": "wrong client"}],
            "asked_to_submit": {"cutoff": "2026-10-31", "asked_by": "Lena Hofer"},
            "something_new": true
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_service_shape_reads() {
        let context = service_context();
        assert_eq!(context.currency(), Currency::EUR);
        assert!(context.allows_tag("team"));
        assert!(context.allows_tag("feature"));
        assert!(!context.allows_tag("purpose"));
        assert!(!context.allows_tag("Team"));
        assert_eq!(context.rules.len(), 1);
        assert_eq!(context.my_rules.first().unwrap().name, "my spikes");
        assert_eq!(
            context
                .asked_to_submit
                .as_ref()
                .unwrap()
                .asked_by
                .as_deref(),
            Some("Lena Hofer")
        );
        let prices = context.price_events(Currency::EUR).unwrap();
        assert_eq!(prices.len(), 1);
        assert!(context.price_events(Currency::USD).is_ok());
        let submissions = context.submission_events().unwrap();
        assert_eq!(submissions.len(), 2);
        let state = State::fold(&submissions, OffsetDateTime::now_utc(), Currency::EUR).unwrap();
        let submission = state.submissions().first().unwrap();
        assert_eq!(submission.status.as_str(), "rejected");
    }

    #[test]
    fn without_an_organization_any_tag_name_goes() {
        let context = Context::default();
        assert_eq!(context.currency(), Currency::USD);
        assert!(context.allows_tag("purpose"));
        assert!(!context.allows_tag("not a name"));
        assert!(context.price_events(Currency::USD).unwrap().is_empty());
        assert!(context.submission_events().unwrap().is_empty());
    }

    #[test]
    fn broken_price_rows_are_refused() {
        let context = Context {
            price_table: vec![PriceLine {
                provider: "anthropic".into(),
                model: "x".into(),
                kind: TokenKind::Input,
                per_million: "3".into(),
                valid_from: "soon".into(),
                source: None,
            }],
            ..Context::default()
        };
        assert!(context.price_events(Currency::USD).is_err());
    }

    #[test]
    fn an_approved_submission_closes_its_window() {
        let context = Context {
            submissions: vec![
                SubmissionLine {
                    id: "sub-1".into(),
                    from: "2026-10-01".into(),
                    to: "2026-10-05".into(),
                    status: Some("approved".into()),
                    reason: None,
                },
                SubmissionLine {
                    id: "sub-2".into(),
                    from: "2026-10-06".into(),
                    to: "2026-10-06".into(),
                    status: None,
                    reason: None,
                },
            ],
            ..Context::default()
        };
        let events = context.submission_events().unwrap();
        assert_eq!(events.len(), 3);
        let state = State::fold(&events, OffsetDateTime::now_utc(), Currency::USD).unwrap();
        let status: Vec<&str> = state
            .submissions()
            .iter()
            .map(|submission| submission.status.as_str())
            .collect();
        assert_eq!(status, ["approved", "submitted"]);
    }

    #[test]
    fn submissions_with_broken_dates_are_left_out() {
        let context = Context {
            submissions: vec![
                SubmissionLine {
                    id: "sub-1".into(),
                    from: "soon".into(),
                    to: "2026-10-05".into(),
                    status: None,
                    reason: None,
                },
                SubmissionLine {
                    id: "sub-2".into(),
                    from: "2026-10-09".into(),
                    to: "2026-10-05".into(),
                    status: None,
                    reason: None,
                },
            ],
            ..Context::default()
        };
        assert!(context.submission_events().unwrap().is_empty());
    }
}
