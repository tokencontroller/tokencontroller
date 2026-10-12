//! The monthly close: what stops a period from closing, and what a closed
//! period keeps (ADR 0007).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::entry::TokenEntry;
use crate::history::Version;
use crate::period::Period;
use crate::status::Status;
use crate::tags::TagValue;

/// A closed period: the versions every figure on its cost sheet is computed
/// with, and who closed it when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedPeriod {
    pub period: Period,
    pub price_table: Version,
    pub tag_map: Version,
    pub closed_by: TagValue,
    #[serde(with = "time::serde::rfc3339")]
    pub closed_at: OffsetDateTime,
}

/// What a controller sees before closing a period.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseCheck {
    /// With approval on: token entries that are submitted or rejected but
    /// not approved. The period cannot close while there are any.
    pub unapproved: usize,
    /// Token entries without an agent manager. They show as an error until
    /// someone takes them.
    pub without_agent_manager: usize,
    /// Rejected token entries. They do not count until fixed; fixed after
    /// the close, they become late adjustments.
    pub rejected: usize,
}

impl CloseCheck {
    /// Looks at a period's token entries. Entries from other periods are
    /// left out.
    #[must_use]
    pub fn of<'a, I>(period: Period, approval_on: bool, entries: I) -> Self
    where
        I: IntoIterator<Item = &'a TokenEntry>,
    {
        let mut check = Self::default();
        for entry in entries {
            if entry.period() != period || entry.status == Status::Draft {
                continue;
            }
            if approval_on && matches!(entry.status, Status::Submitted | Status::Rejected) {
                check.unapproved = check.unapproved.saturating_add(1);
            }
            if entry.status == Status::Rejected {
                check.rejected = check.rejected.saturating_add(1);
            }
            if entry.agent_manager.is_none() {
                check.without_agent_manager = check.without_agent_manager.saturating_add(1);
            }
        }
        check
    }

    /// Whether the period can close: with approval on, every token entry in
    /// it is approved.
    #[must_use]
    pub const fn can_close(&self) -> bool {
        self.unapproved == 0
    }
}

/// The status a token entry takes when its period closes. Approved and
/// submitted ones are locked; drafts and rejected ones stay as they are and
/// reach the closed period only as late adjustments.
#[must_use]
pub const fn status_after_close(status: Status) -> Status {
    match status {
        Status::Submitted | Status::Approved | Status::Locked => Status::Locked,
        Status::Draft => Status::Draft,
        Status::Rejected => Status::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Signals;
    use crate::status::Arrival;
    use crate::tags::Tags;
    use time::macros::datetime;

    fn entry(status: Status, at: OffsetDateTime, manager: bool) -> TokenEntry {
        TokenEntry {
            agent_harness: "Claude Code".to_owned(),
            agent_name: None,
            agent_manager: manager.then(|| TagValue::new("mara").unwrap()),
            session_id: None,
            started_at: at,
            ended_at: at,
            summary: None,
            arrival: Arrival::TokenSheet,
            status,
            tags: Tags::new(),
            configured: Tags::new(),
            signals: Signals::default(),
            segments: Vec::new(),
            hand_split: Vec::new(),
        }
    }

    #[test]
    fn with_approval_on_every_entry_must_be_approved() {
        let october = Period::parse("2026-10").unwrap();
        let in_october = datetime!(2026-10-20 10:00 UTC);
        let entries = vec![
            entry(Status::Approved, in_october, true),
            entry(Status::Submitted, in_october, true),
            entry(Status::Rejected, in_october, true),
            entry(Status::Draft, in_october, false),
            entry(Status::Submitted, datetime!(2026-11-01 00:00 UTC), true),
            entry(Status::Locked, in_october, false),
        ];
        let check = CloseCheck::of(october, true, &entries);
        assert_eq!(
            check,
            CloseCheck {
                unapproved: 2,
                without_agent_manager: 1,
                rejected: 1,
            }
        );
        assert!(!check.can_close());
        let without_approval = CloseCheck::of(october, false, &entries);
        assert_eq!(without_approval.unapproved, 0);
        assert_eq!(without_approval.rejected, 1);
        assert!(without_approval.can_close());
    }

    #[test]
    fn closing_locks_what_counted() {
        assert_eq!(status_after_close(Status::Submitted), Status::Locked);
        assert_eq!(status_after_close(Status::Approved), Status::Locked);
        assert_eq!(status_after_close(Status::Locked), Status::Locked);
        assert_eq!(status_after_close(Status::Draft), Status::Draft);
        assert_eq!(status_after_close(Status::Rejected), Status::Rejected);
    }

    #[test]
    fn a_closed_period_names_its_versions() {
        let closed = ClosedPeriod {
            period: Period::parse("2026-10").unwrap(),
            price_table: Version::new(3).unwrap(),
            tag_map: Version::FIRST,
            closed_by: TagValue::new("priya").unwrap(),
            closed_at: datetime!(2026-11-28 16:00 UTC),
        };
        let json = serde_json::to_string(&closed).unwrap();
        assert!(json.contains(r#""price_table":3"#));
        assert_eq!(serde_json::from_str::<ClosedPeriod>(&json).unwrap(), closed);
    }
}
