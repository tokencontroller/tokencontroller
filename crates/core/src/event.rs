//! The append-only log: events, their ids, and the date ranges reports ask
//! about.
//!
//! Nothing in the log is edited or deleted. A correction is a new event that
//! supersedes or reverses an older one (a reversal). Every figure is a fold
//! of the log up to a date, the "as of" date: see [`crate::State`].

use std::fmt;

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::access::Grant;
use crate::bill::Bill;
use crate::estimate::{Accept, Estimate};
use crate::message::Message;
use crate::plan::Share;
use crate::price::PriceRow;
use crate::rules::Rule;
use crate::split::Split;
use crate::submit::{Approval, Submit};
use crate::tags::TagValue;
use crate::values::{AttributeValue, PersonAttribute, TagValueRecord};

/// An event id that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not an event id: use 1 to 200 characters without spaces at either end")]
pub struct EventIdError(pub String);

/// The id of one event in the log, given by the door that recorded it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EventId(String);

impl EventId {
    /// Reads an event id.
    ///
    /// # Errors
    ///
    /// [`EventIdError`] if it is empty, longer than 200 characters, or
    /// starts or ends with a space.
    pub fn new(id: &str) -> Result<Self, EventIdError> {
        if id.is_empty() || id.trim() != id || id.chars().count() > 200 {
            Err(EventIdError(id.to_owned()))
        } else {
            Ok(Self(id.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EventId {
    type Error = EventIdError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(&id)
    }
}

impl From<EventId> for String {
    fn from(id: EventId) -> Self {
        id.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A range of dates, both ends included, in UTC. Windows `w` of the
/// formulas, bill ranges `r`, estimate ranges and share ranges are all date
/// ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DateRange {
    pub from: Date,
    pub to: Date,
}

/// A date range whose end is before its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the range ends on {to} before it starts on {from}")]
pub struct RangeError {
    pub from: Date,
    pub to: Date,
}

impl DateRange {
    /// A range from one date to another, both included.
    ///
    /// # Errors
    ///
    /// [`RangeError`] if `to` is before `from`.
    pub fn new(from: Date, to: Date) -> Result<Self, RangeError> {
        if to < from {
            Err(RangeError { from, to })
        } else {
            Ok(Self { from, to })
        }
    }

    /// One day.
    #[must_use]
    pub const fn day(date: Date) -> Self {
        Self {
            from: date,
            to: date,
        }
    }

    /// The number of days in the range, at least 1.
    #[must_use]
    pub fn days(self) -> u64 {
        let between = self
            .to
            .to_julian_day()
            .saturating_sub(self.from.to_julian_day());
        u64::try_from(between).unwrap_or(0).saturating_add(1)
    }

    /// Whether a date is inside the range.
    #[must_use]
    pub fn contains(self, date: Date) -> bool {
        self.from <= date && date <= self.to
    }

    /// Whether a moment falls on a date inside the range, in UTC.
    #[must_use]
    pub fn contains_moment(self, moment: OffsetDateTime) -> bool {
        self.contains(moment.to_offset(time::UtcOffset::UTC).date())
    }

    /// The days both ranges share, or `None` if they share none.
    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let from = self.from.max(other.from);
        let to = self.to.min(other.to);
        Self::new(from, to).ok()
    }
}

impl fmt::Display for DateRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} to {}", self.from, self.to)
    }
}

/// What one event records: one of the source records, assignments and other
/// events of the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Record {
    /// Source record 1: one model call inside an agent session.
    Message(Box<Message>),
    /// Source record 2: a provider's bill for metered usage.
    Bill(Bill),
    /// Source record 3: an estimate of what work will cost.
    Estimate(Estimate),
    /// Source record 4: a tag value with its defaults.
    TagValue(TagValueRecord),
    /// Source record 5: a person's share of a project's plan.
    Share(Share),
    /// Source record 6: one row of the price table.
    PriceRow(PriceRow),
    /// Source record 7: an attribute value with its defaults.
    AttributeValue(AttributeValue),
    /// Puts an attribute value on a person. A later one for the same person
    /// and attribute name supersedes it.
    PersonAttribute(PersonAttribute),
    /// Assignment 8: how a session or a message divides over tag sets. A
    /// later split on the same target supersedes it.
    Split(Split),
    /// Assignment 9: makes one estimate the plan for its tag set. A later
    /// accept for the same tag set supersedes it.
    Accept(Accept),
    /// Assignment 10: a capability on a filter for a person.
    Grant(Grant),
    /// The organization's cutoff date. The latest one is in force.
    Cutoff(Date),
    /// A person submits all their own messages inside a window.
    Submit(Submit),
    /// Approves or rejects a submission.
    Approval(Approval),
    /// A rule that turns signals into a split.
    Rule(Rule),
    /// Reverses the event named, as if it had never been recorded.
    Reversal(EventId),
}

impl Record {
    /// The word the docs use for this kind of event.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Message(_) => "message",
            Self::Bill(_) => "bill",
            Self::Estimate(_) => "estimate",
            Self::TagValue(_) => "tag value",
            Self::Share(_) => "share",
            Self::PriceRow(_) => "price table row",
            Self::AttributeValue(_) => "attribute value",
            Self::PersonAttribute(_) => "person attribute",
            Self::Split(_) => "split",
            Self::Accept(_) => "accept",
            Self::Grant(_) => "grant",
            Self::Cutoff(_) => "cutoff",
            Self::Submit(_) => "submit",
            Self::Approval(_) => "approval",
            Self::Rule(_) => "rule",
            Self::Reversal(_) => "reversal",
        }
    }
}

/// One event of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    /// When the event entered the log. The fold "as of" a moment takes the
    /// events recorded at or before it.
    #[serde(with = "time::serde::rfc3339")]
    pub recorded_at: OffsetDateTime,
    /// An older event this one replaces. The older one stays in the log and
    /// stops counting.
    #[serde(default)]
    pub supersedes: Option<EventId>,
    /// The person who recorded the event, where a person did.
    #[serde(default)]
    pub recorded_by: Option<TagValue>,
    pub record: Record,
}

impl Event {
    /// An event with no supersedes pointer and no recorder.
    ///
    /// # Errors
    ///
    /// [`EventIdError`] if the id cannot be used.
    pub fn new(
        id: &str,
        recorded_at: OffsetDateTime,
        record: Record,
    ) -> Result<Self, EventIdError> {
        Ok(Self {
            id: EventId::new(id)?,
            recorded_at,
            supersedes: None,
            recorded_by: None,
            record,
        })
    }

    /// The same event, superseding an older one.
    #[must_use]
    pub fn superseding(mut self, older: EventId) -> Self {
        self.supersedes = Some(older);
        self
    }

    /// The same event, recorded by a person.
    #[must_use]
    pub fn by(mut self, person: TagValue) -> Self {
        self.recorded_by = Some(person);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn event_ids_are_short_trimmed_text() {
        assert_eq!(EventId::new("evt-1").unwrap().as_str(), "evt-1");
        assert_eq!(EventId::new("e").unwrap().to_string(), "e");
        assert!(EventId::new(&"x".repeat(200)).is_ok());
        for bad in ["", " a", "a ", &"x".repeat(201)] {
            assert_eq!(EventId::new(bad), Err(EventIdError(bad.to_owned())));
        }
        assert!(serde_json::from_str::<EventId>(r#""""#).is_err());
        assert_eq!(
            serde_json::to_string(&EventId::new("e1").unwrap()).unwrap(),
            r#""e1""#
        );
    }

    #[test]
    fn a_range_counts_its_days_both_ends_included() {
        let march = DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap();
        assert_eq!(march.days(), 31);
        assert_eq!(DateRange::day(date!(2026 - 03 - 01)).days(), 1);
        let hundred = DateRange::new(date!(2026 - 02 - 01), date!(2026 - 05 - 11)).unwrap();
        assert_eq!(hundred.days(), 100);
        assert_eq!(march.to_string(), "2026-03-01 to 2026-03-31");
    }

    #[test]
    fn a_range_cannot_end_before_it_starts() {
        assert_eq!(
            DateRange::new(date!(2026 - 03 - 02), date!(2026 - 03 - 01)),
            Err(RangeError {
                from: date!(2026 - 03 - 02),
                to: date!(2026 - 03 - 01),
            })
        );
        assert!(DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 01)).is_ok());
    }

    #[test]
    fn a_range_contains_its_ends_and_moments_in_utc() {
        let march = DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap();
        assert!(march.contains(date!(2026 - 03 - 01)));
        assert!(march.contains(date!(2026 - 03 - 31)));
        assert!(!march.contains(date!(2026 - 02 - 28)));
        assert!(!march.contains(date!(2026 - 04 - 01)));
        assert!(march.contains_moment(datetime!(2026-03-31 23:59 UTC)));
        assert!(!march.contains_moment(datetime!(2026-04-01 00:00 UTC)));
        // 1 April 01:00 at +02:00 is still 31 March in UTC.
        assert!(march.contains_moment(datetime!(2026-04-01 01:00 +02:00)));
        assert!(!march.contains_moment(datetime!(2026-02-28 21:00 -02:00)));
    }

    #[test]
    fn ranges_intersect_on_the_days_they_share() {
        let march = DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap();
        let hundred = DateRange::new(date!(2026 - 02 - 01), date!(2026 - 05 - 11)).unwrap();
        assert_eq!(march.intersection(hundred), Some(march));
        assert_eq!(hundred.intersection(march), Some(march));
        let late = DateRange::new(date!(2026 - 03 - 20), date!(2026 - 04 - 10)).unwrap();
        assert_eq!(
            march.intersection(late),
            Some(DateRange::new(date!(2026 - 03 - 20), date!(2026 - 03 - 31)).unwrap())
        );
        let april = DateRange::new(date!(2026 - 04 - 01), date!(2026 - 04 - 30)).unwrap();
        assert_eq!(march.intersection(april), None);
        assert_eq!(
            march.intersection(DateRange::day(date!(2026 - 03 - 31))),
            Some(DateRange::day(date!(2026 - 03 - 31)))
        );
    }

    #[test]
    fn events_carry_their_pointers_and_recorder() {
        let event = Event::new(
            "c1",
            datetime!(2026-03-01 00:00 UTC),
            Record::Cutoff(date!(2026 - 03 - 01)),
        )
        .unwrap();
        assert_eq!(event.supersedes, None);
        assert_eq!(event.recorded_by, None);
        assert_eq!(event.record.kind(), "cutoff");
        let later = event
            .clone()
            .superseding(EventId::new("c0").unwrap())
            .by(TagValue::new("mara").unwrap());
        assert_eq!(later.supersedes.as_ref().unwrap().as_str(), "c0");
        assert_eq!(later.recorded_by.as_ref().unwrap().as_str(), "mara");
        assert!(
            Event::new(
                "",
                datetime!(2026-03-01 00:00 UTC),
                Record::Cutoff(date!(2026 - 03 - 01))
            )
            .is_err()
        );
        let json = serde_json::to_string(&later).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), later);
    }

    #[test]
    fn records_read_and_write_as_tagged_json() {
        let cutoff = Record::Cutoff(date!(2026 - 03 - 01));
        let json = serde_json::to_string(&cutoff).unwrap();
        assert_eq!(json, r#"{"cutoff":"2026-03-01"}"#);
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), cutoff);
        assert_eq!(cutoff.kind(), "cutoff");
        let grant: Record = serde_json::from_str(
            r#"{"grant":{"person":"lea","capability":"view","filter":{"client":"acme"}}}"#,
        )
        .unwrap();
        assert_eq!(grant.kind(), "grant");
        let share: Record = serde_json::from_str(
            r#"{"share":{"person":"mara","project":"shop","fraction_basis_points":6000,"from":"2026-03-01"}}"#,
        )
        .unwrap();
        assert_eq!(share.kind(), "share");
        assert!(serde_json::from_str::<Record>(r#"{"close":"2026-03"}"#).is_err());
    }

    #[test]
    fn every_kind_of_record_has_its_word() {
        let reversal = Record::Reversal(EventId::new("x").unwrap());
        assert_eq!(reversal.kind(), "reversal");
        assert_eq!(
            serde_json::to_string(&reversal).unwrap(),
            r#"{"reversal":"x"}"#
        );
    }
}
