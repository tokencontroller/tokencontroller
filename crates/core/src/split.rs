//! Splits (assignment 8): how a session or a message divides over tag
//! sets, and the labels reports derive from a split.
//!
//! A split is a list of (fraction, tag set); the fractions sum to 1. It is
//! kept as whole-number weights, and the message's cost is divided by them
//! with [`Money::allocate`], so the parts always add up to the whole.

use serde::{Deserialize, Serialize};

use crate::message::{MessageId, SessionId};
use crate::money::{Money, MoneyError};
use crate::tags::{TagName, Tags};

/// What a split is on: every message of a session, or one message. A split
/// on a message wins over one on its session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitTarget {
    Session(SessionId),
    Message {
        session_id: SessionId,
        message_id: MessageId,
    },
}

/// One part of a split: a weight and the tags it goes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitPart {
    /// The part's weight, such as 60 for 60 percent. Fractions are weights
    /// over their sum.
    pub weight: u64,
    pub tags: Tags,
}

/// A split that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SplitError {
    #[error("a split needs at least one part with a weight above zero")]
    NoWeights,
    #[error("a split may not name {0}: the person comes from the message")]
    NamesPerson(TagName),
}

/// A split assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub target: SplitTarget,
    pub parts: Vec<SplitPart>,
}

impl Split {
    /// One part, 100 percent, on a tag set.
    #[must_use]
    pub fn whole(target: SplitTarget, tags: Tags) -> Self {
        Self {
            target,
            parts: vec![SplitPart { weight: 1, tags }],
        }
    }

    /// Checks that the parts can divide an amount and name no person.
    ///
    /// # Errors
    ///
    /// [`SplitError::NoWeights`] if no part has a weight above zero, or
    /// [`SplitError::NamesPerson`] if a part names `person`.
    pub fn check(&self) -> Result<(), SplitError> {
        if !self.parts.iter().any(|part| part.weight > 0) {
            return Err(SplitError::NoWeights);
        }
        if self
            .parts
            .iter()
            .any(|part| part.tags.contains(&TagName::person()))
        {
            return Err(SplitError::NamesPerson(TagName::person()));
        }
        Ok(())
    }

    /// The weights of the parts, in order.
    #[must_use]
    pub fn weights(&self) -> Vec<u64> {
        self.parts.iter().map(|part| part.weight).collect()
    }

    /// Divides an amount over the parts by their weights, so the parts add
    /// up to the amount exactly.
    ///
    /// # Errors
    ///
    /// As [`Money::allocate`].
    pub fn allocate(&self, amount: Money) -> Result<Vec<Money>, MoneyError> {
        amount.allocate(&self.weights())
    }
}

/// How a split got there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignedBy {
    /// A person recorded the split.
    Hand,
    /// A rule matched a signal.
    Rule,
    /// Nobody decided: the message's own tags, with equal shares when the
    /// session names several features.
    Default,
}

/// The label reports derive from a split's tags: direct work, project
/// overhead or organization overhead. A split still has its person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    /// The split has a feature.
    Direct,
    /// The split has a project and no feature.
    ProjectOverhead,
    /// The split has neither.
    OrganizationOverhead,
}

impl Label {
    /// The label of a closed tag set.
    #[must_use]
    pub fn of(tags: &Tags) -> Self {
        if tags.contains(&TagName::feature()) {
            Self::Direct
        } else if tags.contains(&TagName::project()) {
            Self::ProjectOverhead
        } else {
            Self::OrganizationOverhead
        }
    }

    /// The label as reports write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ProjectOverhead => "project overhead",
            Self::OrganizationOverhead => "organization overhead",
        }
    }

    /// Every label, in the order reports list them.
    pub const ALL: [Self; 3] = [
        Self::Direct,
        Self::ProjectOverhead,
        Self::OrganizationOverhead,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn session() -> SplitTarget {
        SplitTarget::Session(SessionId::new("s2").unwrap())
    }

    fn sixty_forty() -> Split {
        Split {
            target: session(),
            parts: vec![
                SplitPart {
                    weight: 60,
                    tags: tags(&["feature=KD-1"]),
                },
                SplitPart {
                    weight: 40,
                    tags: tags(&["feature=KD-2"]),
                },
            ],
        }
    }

    #[test]
    fn a_split_divides_an_amount_by_its_weights() {
        let usd = |text| Money::parse(text, Currency::USD).unwrap();
        assert_eq!(sixty_forty().weights(), vec![60, 40]);
        assert_eq!(
            sixty_forty().allocate(usd("300")).unwrap(),
            vec![usd("180"), usd("120")]
        );
        let whole = Split::whole(session(), tags(&["project=shop"]));
        assert_eq!(whole.allocate(usd("100")).unwrap(), vec![usd("100")]);
        assert_eq!(whole.parts.len(), 1);
    }

    #[test]
    fn a_split_needs_a_weight_and_names_no_person() {
        assert_eq!(sixty_forty().check(), Ok(()));
        let none = Split {
            target: session(),
            parts: vec![SplitPart {
                weight: 0,
                tags: Tags::new(),
            }],
        };
        assert_eq!(none.check(), Err(SplitError::NoWeights));
        assert!(none.allocate(Money::zero(Currency::USD)).is_err());
        let empty = Split {
            target: session(),
            parts: Vec::new(),
        };
        assert_eq!(empty.check(), Err(SplitError::NoWeights));
        let person = Split::whole(session(), tags(&["person=tom"]));
        assert_eq!(
            person.check(),
            Err(SplitError::NamesPerson(TagName::person()))
        );
    }

    #[test]
    fn a_message_target_is_distinct_from_its_session() {
        let message = SplitTarget::Message {
            session_id: SessionId::new("s2").unwrap(),
            message_id: MessageId::new("m1").unwrap(),
        };
        assert_ne!(message, session());
        let json = serde_json::to_string(&message).unwrap();
        assert_eq!(json, r#"{"message":{"session_id":"s2","message_id":"m1"}}"#);
        assert_eq!(
            serde_json::to_string(&session()).unwrap(),
            r#"{"session":"s2"}"#
        );
        assert_eq!(
            serde_json::from_str::<Split>(&serde_json::to_string(&sixty_forty()).unwrap()).unwrap(),
            sixty_forty()
        );
    }

    #[test]
    fn a_feature_is_direct_a_project_alone_is_project_overhead() {
        assert_eq!(
            Label::of(&tags(&["feature=KD-1", "project=shop"])),
            Label::Direct
        );
        assert_eq!(Label::of(&tags(&["feature=KD-1"])), Label::Direct);
        assert_eq!(
            Label::of(&tags(&["project=shop", "client=acme"])),
            Label::ProjectOverhead
        );
        assert_eq!(
            Label::of(&tags(&["person=tom", "team=web"])),
            Label::OrganizationOverhead
        );
        assert_eq!(Label::of(&Tags::new()), Label::OrganizationOverhead);
    }

    #[test]
    fn labels_are_written_in_plain_words() {
        let words: Vec<&str> = Label::ALL.iter().map(|label| label.as_str()).collect();
        assert_eq!(
            words,
            vec!["direct", "project overhead", "organization overhead"]
        );
        assert_eq!(
            serde_json::to_string(&Label::ProjectOverhead).unwrap(),
            r#""project_overhead""#
        );
        assert_eq!(
            serde_json::to_string(&AssignedBy::Hand).unwrap(),
            r#""hand""#
        );
        assert!(AssignedBy::Hand < AssignedBy::Rule && AssignedBy::Rule < AssignedBy::Default);
    }
}
