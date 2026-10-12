//! The settings of one organization that the core computes with.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::directory::Directory;
use crate::money::Currency;
use crate::rules::{Rule, SeveralKeys};
use crate::status::CostSource;
use crate::tag_map::TagMap;
use crate::tags::{TagName, TagValue};

/// One customer of Token Controller: its settings, people, projects,
/// tickets and rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Organization {
    /// The one currency the organization computes in.
    pub reporting_currency: Currency,
    pub cost_source: CostSource,
    /// Whether people managers approve token entries. With approval on, a
    /// period cannot close while a token entry in it is unapproved, and only
    /// approved cost counts.
    #[serde(default)]
    pub approval: bool,
    /// The organization's own tag names. `person`, `ticket` and `project`
    /// are always allowed; tags with any other name are dropped.
    #[serde(default)]
    pub tag_names: BTreeSet<TagName>,
    #[serde(default)]
    pub tag_map: TagMap,
    #[serde(default)]
    pub directory: Directory,
    /// Organization rules, in order. They run before personal rules.
    #[serde(default = "Rule::defaults")]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub several_keys: SeveralKeys,
    /// Whether the service tries a summary match when no rule finds a
    /// ticket.
    #[serde(default = "summary_match_default")]
    pub summary_match: bool,
    /// Ticket keys that matched a pattern but that the last ticket sync did
    /// not find. Rules and the summary match run again without them.
    #[serde(default)]
    pub missing_tickets: BTreeSet<TagValue>,
}

const fn summary_match_default() -> bool {
    true
}

impl Organization {
    /// A new organization with the default tag map and rules.
    #[must_use]
    pub fn new(reporting_currency: Currency, cost_source: CostSource) -> Self {
        Self {
            reporting_currency,
            cost_source,
            approval: false,
            tag_names: BTreeSet::new(),
            tag_map: TagMap::default(),
            directory: Directory::default(),
            rules: Rule::defaults(),
            several_keys: SeveralKeys::default(),
            summary_match: summary_match_default(),
            missing_tickets: BTreeSet::new(),
        }
    }

    /// Whether a tag name may be used: a built-in tag or one on the
    /// organization's list.
    #[must_use]
    pub fn allows(&self, name: &TagName) -> bool {
        name.is_built_in() || self.tag_names.contains(name)
    }

    /// The rules that run for a person: the organization's first, then the
    /// person's own.
    pub fn rules_for<'a>(&'a self, person: Option<&TagValue>) -> impl Iterator<Item = &'a Rule> {
        let personal = person
            .and_then(|person| self.directory.people.get(person))
            .map(|person| person.rules.as_slice())
            .unwrap_or_default();
        self.rules.iter().chain(personal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::Person;
    use crate::rules::Action;
    use crate::tags::Tags;

    #[test]
    fn a_new_organization_starts_with_the_defaults() {
        let organization = Organization::new(Currency::EUR, CostSource::ApiKey);
        assert_eq!(organization.rules, Rule::defaults());
        assert_eq!(organization.tag_map, TagMap::default());
        assert!(organization.summary_match);
        assert!(!organization.approval);
        let read: Organization =
            serde_json::from_str(r#"{"reporting_currency": "EUR", "cost_source": "api_key"}"#)
                .unwrap();
        assert_eq!(read, organization);
    }

    #[test]
    fn built_in_and_listed_tag_names_are_allowed() {
        let mut organization = Organization::new(Currency::EUR, CostSource::ApiKey);
        organization.tag_names.insert(TagName::new("team").unwrap());
        assert!(organization.allows(&TagName::person()));
        assert!(organization.allows(&TagName::ticket()));
        assert!(organization.allows(&TagName::project()));
        assert!(organization.allows(&TagName::new("team").unwrap()));
        assert!(!organization.allows(&TagName::new("client").unwrap()));
    }

    #[test]
    fn personal_rules_run_after_the_organizations() {
        let mut organization = Organization::new(Currency::EUR, CostSource::ApiKey);
        let mara = TagValue::new("mara").unwrap();
        let personal = Rule {
            name: "my learning".to_owned(),
            when: Vec::new(),
            then: Action::SetTags(Tags::parse(["purpose=learning"]).unwrap()),
        };
        organization.directory.people.insert(
            mara.clone(),
            Person {
                tags: Tags::new(),
                rules: vec![personal.clone()],
            },
        );
        let names: Vec<&str> = organization
            .rules_for(Some(&mara))
            .map(|rule| rule.name.as_str())
            .collect();
        assert_eq!(names.len(), 5);
        assert_eq!(names.last(), Some(&"my learning"));
        assert_eq!(organization.rules_for(None).count(), 4);
        let nobody = TagValue::new("nobody").unwrap();
        assert_eq!(organization.rules_for(Some(&nobody)).count(), 4);
    }
}
