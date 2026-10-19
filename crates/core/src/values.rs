//! Tag values (source record 4) and attribute values (source record 7),
//! and the closure of a tag set under their defaults (formula 2).
//!
//! `T*(s)` is the tag set of a split closed under defaults: a feature brings
//! its project, a project brings its client; the person brings their
//! attributes, and a team brings its department. Tag values are records,
//! never bare strings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::tags::{TagName, TagValue, Tags};

/// A tag value: one value a tag name can take, with the tags it brings by
/// default, such as feature `KD-1` with default project `shop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagValueRecord {
    pub name: TagName,
    pub key: TagValue,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// The id in the source system, such as a Jira issue id.
    #[serde(default)]
    pub source_id: Option<String>,
    /// The status in the source system, such as `done`.
    #[serde(default)]
    pub source_status: Option<String>,
    /// `D(v)`: the tags this value brings, such as `project: shop`.
    #[serde(default)]
    pub defaults: Tags,
}

/// An attribute value: like a tag value, for people, such as team `web`
/// with default department `engineering`. A different word from tags on
/// purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeValue {
    pub name: TagName,
    pub key: TagValue,
    #[serde(default)]
    pub title: Option<String>,
    /// The attributes this value brings, such as `department: engineering`.
    #[serde(default)]
    pub defaults: Tags,
}

/// Puts an attribute value on a person, such as Mara in team `web`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonAttribute {
    pub person: TagValue,
    pub name: TagName,
    pub key: TagValue,
}

/// The tag values and attribute values in force, by name and key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    tags: BTreeMap<(TagName, TagValue), TagValueRecord>,
    attributes: BTreeMap<(TagName, TagValue), AttributeValue>,
    /// Each person's attributes, one value per attribute name.
    people: BTreeMap<TagValue, Tags>,
}

impl Values {
    /// Adds or replaces a tag value.
    pub fn insert_tag_value(&mut self, record: TagValueRecord) {
        self.tags
            .insert((record.name.clone(), record.key.clone()), record);
    }

    /// Adds or replaces an attribute value.
    pub fn insert_attribute_value(&mut self, value: AttributeValue) {
        self.attributes
            .insert((value.name.clone(), value.key.clone()), value);
    }

    /// Puts an attribute value on a person, replacing the person's earlier
    /// value for that attribute name.
    pub fn insert_person_attribute(&mut self, attribute: PersonAttribute) {
        self.people
            .entry(attribute.person)
            .or_default()
            .insert(attribute.name, attribute.key);
    }

    /// Removes a person's value for an attribute name.
    pub fn remove_person_attribute(&mut self, person: &TagValue, name: &TagName) {
        if let Some(attributes) = self.people.get_mut(person) {
            attributes.remove(name);
        }
    }

    /// The tag value record for a name and key, if there is one.
    #[must_use]
    pub fn tag_value(&self, name: &TagName, key: &TagValue) -> Option<&TagValueRecord> {
        self.tags.get(&(name.clone(), key.clone()))
    }

    /// The attribute value for a name and key, if there is one.
    #[must_use]
    pub fn attribute_value(&self, name: &TagName, key: &TagValue) -> Option<&AttributeValue> {
        self.attributes.get(&(name.clone(), key.clone()))
    }

    /// Every tag value, by name then key.
    pub fn tag_values(&self) -> impl Iterator<Item = &TagValueRecord> {
        self.tags.values()
    }

    /// Every attribute value, by name then key.
    pub fn attribute_values(&self) -> impl Iterator<Item = &AttributeValue> {
        self.attributes.values()
    }

    /// The attributes put on a person directly, before defaults.
    #[must_use]
    pub fn attributes_of(&self, person: &TagValue) -> Tags {
        self.people.get(person).cloned().unwrap_or_default()
    }

    /// Whether a key is a value of a tag name.
    #[must_use]
    pub fn is_tag_value(&self, name: &TagName, key: &TagValue) -> bool {
        self.tags.contains_key(&(name.clone(), key.clone()))
    }

    /// `T*(s)`: the tags of a split closed under defaults, with the person
    /// and the person's attributes closed under their defaults. A tag the
    /// split names wins over a default; a default brought earlier wins over
    /// one brought later. The closure is bounded by the number of values,
    /// so a cycle in the defaults cannot loop.
    #[must_use]
    pub fn close(&self, tags: &Tags, person: &TagValue) -> Tags {
        let mut closed = tags.clone();
        closed.insert(TagName::person(), person.clone());
        for (name, key) in self.attributes_of(person).iter() {
            if !closed.contains(name) {
                closed.insert(name.clone(), key.clone());
            }
        }
        let bound = self.tags.len().saturating_add(self.attributes.len());
        for _ in 0..=bound {
            let mut brought = Tags::new();
            for (name, key) in closed.iter() {
                let defaults = self
                    .tag_value(name, key)
                    .map(|record| &record.defaults)
                    .or_else(|| self.attribute_value(name, key).map(|value| &value.defaults));
                let Some(defaults) = defaults else {
                    continue;
                };
                for (default_name, default_key) in defaults.iter() {
                    if !closed.contains(default_name) && !brought.contains(default_name) {
                        brought.insert(default_name.clone(), default_key.clone());
                    }
                }
            }
            if brought.is_empty() {
                break;
            }
            closed = closed.over(&brought);
        }
        closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> TagName {
        TagName::new(text).unwrap()
    }

    fn key(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn tags(pairs: &[&str]) -> Tags {
        Tags::parse(pairs.iter().copied()).unwrap()
    }

    fn tag_value(name_text: &str, key_text: &str, defaults: &[&str]) -> TagValueRecord {
        TagValueRecord {
            name: name(name_text),
            key: key(key_text),
            title: None,
            description: None,
            source_id: None,
            source_status: None,
            defaults: tags(defaults),
        }
    }

    fn worked_month() -> Values {
        let mut values = Values::default();
        values.insert_tag_value(tag_value("feature", "KD-1", &["project=shop"]));
        values.insert_tag_value(tag_value("feature", "KD-2", &["project=shop"]));
        values.insert_tag_value(tag_value("project", "shop", &["client=acme"]));
        values.insert_tag_value(tag_value("client", "acme", &[]));
        values.insert_attribute_value(AttributeValue {
            name: name("team"),
            key: key("web"),
            title: Some("Web".to_owned()),
            defaults: tags(&["department=engineering"]),
        });
        values.insert_person_attribute(PersonAttribute {
            person: key("mara"),
            name: name("team"),
            key: key("web"),
        });
        values.insert_person_attribute(PersonAttribute {
            person: key("tom"),
            name: name("team"),
            key: key("web"),
        });
        values
    }

    #[test]
    fn a_feature_brings_its_project_and_the_project_its_client() {
        let values = worked_month();
        let closed = values.close(&tags(&["feature=KD-1"]), &key("mara"));
        assert_eq!(
            closed,
            tags(&[
                "feature=KD-1",
                "project=shop",
                "client=acme",
                "person=mara",
                "team=web",
                "department=engineering"
            ])
        );
        let project_only = values.close(&tags(&["project=shop"]), &key("tom"));
        assert_eq!(
            project_only,
            tags(&[
                "project=shop",
                "client=acme",
                "person=tom",
                "team=web",
                "department=engineering"
            ])
        );
    }

    #[test]
    fn a_split_with_no_tags_still_has_its_person_and_attributes() {
        let values = worked_month();
        assert_eq!(
            values.close(&Tags::new(), &key("tom")),
            tags(&["person=tom", "team=web", "department=engineering"])
        );
        assert_eq!(
            values.close(&Tags::new(), &key("nobody")),
            tags(&["person=nobody"])
        );
    }

    #[test]
    fn a_named_tag_wins_over_a_default() {
        let values = worked_month();
        let closed = values.close(&tags(&["feature=KD-1", "project=other"]), &key("mara"));
        assert_eq!(closed.get_str("project").unwrap().as_str(), "other");
        // The other project has no client, so none is brought.
        assert_eq!(closed.get_str("client"), None);
        let own_team = values.close(&tags(&["team=data"]), &key("mara"));
        assert_eq!(own_team.get_str("team").unwrap().as_str(), "data");
        assert_eq!(own_team.get_str("department"), None);
    }

    #[test]
    fn an_unknown_key_brings_nothing() {
        let values = worked_month();
        let closed = values.close(&tags(&["feature=KD-9"]), &key("mara"));
        assert_eq!(closed.get_str("project"), None);
        assert!(!values.is_tag_value(&name("feature"), &key("KD-9")));
        assert!(values.is_tag_value(&name("feature"), &key("KD-1")));
        assert!(values.tag_value(&name("feature"), &key("KD-9")).is_none());
        assert!(
            values
                .attribute_value(&name("team"), &key("data"))
                .is_none()
        );
    }

    #[test]
    fn a_cycle_in_the_defaults_ends() {
        let mut values = Values::default();
        values.insert_tag_value(tag_value("a", "1", &["b=1"]));
        values.insert_tag_value(tag_value("b", "1", &["c=1"]));
        values.insert_tag_value(tag_value("c", "1", &["a=1"]));
        let closed = values.close(&tags(&["a=1"]), &key("p"));
        assert_eq!(closed, tags(&["a=1", "b=1", "c=1", "person=p"]));
        let from_b = values.close(&tags(&["b=1"]), &key("p"));
        assert_eq!(from_b, tags(&["a=1", "b=1", "c=1", "person=p"]));
    }

    #[test]
    fn a_later_record_for_the_same_key_replaces_the_earlier_one() {
        let mut values = worked_month();
        values.insert_tag_value(tag_value("feature", "KD-1", &["project=store"]));
        assert_eq!(
            values
                .tag_value(&name("feature"), &key("KD-1"))
                .unwrap()
                .defaults,
            tags(&["project=store"])
        );
        assert_eq!(values.tag_values().count(), 4);
        values.insert_attribute_value(AttributeValue {
            name: name("team"),
            key: key("web"),
            title: None,
            defaults: Tags::new(),
        });
        assert_eq!(values.attribute_values().count(), 1);
        assert_eq!(
            values.close(&Tags::new(), &key("mara")),
            tags(&["person=mara", "team=web"])
        );
    }

    #[test]
    fn a_persons_attribute_is_replaced_or_removed() {
        let mut values = worked_month();
        values.insert_person_attribute(PersonAttribute {
            person: key("mara"),
            name: name("team"),
            key: key("data"),
        });
        assert_eq!(values.attributes_of(&key("mara")), tags(&["team=data"]));
        values.remove_person_attribute(&key("mara"), &name("team"));
        assert_eq!(values.attributes_of(&key("mara")), Tags::new());
        values.remove_person_attribute(&key("nobody"), &name("team"));
        assert_eq!(values.attributes_of(&key("tom")), tags(&["team=web"]));
    }

    #[test]
    fn values_read_and_write_as_json() {
        let json = r#"{"name": "feature", "key": "KD-1", "title": "Login",
            "source_id": "10001", "defaults": {"project": "shop"}}"#;
        let record: TagValueRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.title.as_deref(), Some("Login"));
        assert_eq!(record.source_status, None);
        assert_eq!(record.defaults, tags(&["project=shop"]));
        let attribute: AttributeValue = serde_json::from_str(
            r#"{"name": "team", "key": "web", "defaults": {"department": "engineering"}}"#,
        )
        .unwrap();
        assert_eq!(attribute.title, None);
        assert_eq!(attribute.defaults, tags(&["department=engineering"]));
        let person: PersonAttribute =
            serde_json::from_str(r#"{"person": "mara", "name": "team", "key": "web"}"#).unwrap();
        assert_eq!(person.person.as_str(), "mara");
    }
}
