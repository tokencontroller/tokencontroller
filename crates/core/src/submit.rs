//! Submissions: a person submits all their own messages inside a window,
//! and approval of a submission, where the organization asks for it.

use serde::{Deserialize, Serialize};

use crate::event::{DateRange, EventId};
use crate::tags::TagValue;

/// A person submits all their own messages inside a window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submit {
    pub person: TagValue,
    pub window: DateRange,
}

/// The decision on a submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approved,
    Rejected { reason: String },
}

/// Approves or rejects a submission. A later approval of the same
/// submission supersedes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    /// The id of the submit event.
    pub submission: EventId,
    pub decision: Decision,
}

/// Where a submission stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    /// Submitted, with no decision yet. Where approval is optional, this is
    /// as far as it goes.
    Submitted,
    Approved,
    Rejected {
        reason: String,
    },
}

impl SubmissionStatus {
    /// The status in one word, as reports write it.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Approved => "approved",
            Self::Rejected { .. } => "rejected",
        }
    }
}

/// A submission with its status, as the fold reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submission {
    pub id: EventId,
    pub person: TagValue,
    pub window: DateRange,
    pub status: SubmissionStatus,
}

impl Submission {
    /// Whether a message of the person at a moment is inside this
    /// submission.
    #[must_use]
    pub fn covers(&self, person: &TagValue, at: time::OffsetDateTime) -> bool {
        self.person == *person && self.window.contains_moment(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn submission(status: SubmissionStatus) -> Submission {
        Submission {
            id: EventId::new("sub-1").unwrap(),
            person: TagValue::new("mara").unwrap(),
            window: DateRange::new(date!(2026 - 03 - 01), date!(2026 - 03 - 31)).unwrap(),
            status,
        }
    }

    #[test]
    fn a_submission_covers_the_persons_messages_in_its_window() {
        let submission = submission(SubmissionStatus::Submitted);
        let mara = TagValue::new("mara").unwrap();
        assert!(submission.covers(&mara, datetime!(2026-03-15 10:00 UTC)));
        assert!(!submission.covers(&mara, datetime!(2026-04-01 00:00 UTC)));
        assert!(!submission.covers(
            &TagValue::new("tom").unwrap(),
            datetime!(2026-03-15 10:00 UTC)
        ));
    }

    #[test]
    fn statuses_are_written_in_one_word() {
        assert_eq!(SubmissionStatus::Submitted.as_str(), "submitted");
        assert_eq!(SubmissionStatus::Approved.as_str(), "approved");
        assert_eq!(
            SubmissionStatus::Rejected {
                reason: "wrong project".to_owned()
            }
            .as_str(),
            "rejected"
        );
        let approval: Approval = serde_json::from_str(
            r#"{"submission": "sub-1", "decision": {"rejected": {"reason": "wrong project"}}}"#,
        )
        .unwrap();
        assert_eq!(
            approval.decision,
            Decision::Rejected {
                reason: "wrong project".to_owned()
            }
        );
        assert_eq!(
            serde_json::to_string(&Decision::Approved).unwrap(),
            r#""approved""#
        );
        let submit: Submit = serde_json::from_str(
            r#"{"person": "mara", "window": {"from": "2026-03-01", "to": "2026-03-31"}}"#,
        )
        .unwrap();
        assert_eq!(submit.window.days(), 31);
    }
}
