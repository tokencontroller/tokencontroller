//! Who may do what: `can(person, capability, F)` over the grants in the
//! fold (ADR 0023). Handlers call [`require`] before they append an event,
//! and [`view_filters`] to answer inside what the caller may view.

use time::OffsetDateTime;
use tokencontroller_core::state::State;
use tokencontroller_core::{Capability, Filter, TagValue, Tags, price::date_of};

use crate::error::ApiError;

/// The capability in the words of the docs.
#[must_use]
pub const fn capability_word(capability: Capability) -> &'static str {
    match capability {
        Capability::View => "view",
        Capability::Record => "record",
        Capability::Assign => "assign",
        Capability::Approve => "approve",
        Capability::Plan => "plan",
        Capability::Grant => "grant",
        Capability::Adjust => "adjust",
        Capability::Cutoff => "cutoff",
    }
}

/// A filter in words: `client acme · team web`, or `the whole organization`.
#[must_use]
pub fn filter_words(filter: &Filter) -> String {
    if filter.is_everything() {
        "the whole organization".to_owned()
    } else {
        filter.0.to_string()
    }
}

/// Refuses a caller whose grants do not cover `capability` on `filter`.
/// Record needs no grant.
///
/// # Errors
///
/// [`ApiError::NoCapability`] naming the capability and the filter.
pub fn require(
    state: &State,
    person: &TagValue,
    capability: Capability,
    filter: &Filter,
) -> Result<(), ApiError> {
    if state.can(person, capability, filter) {
        Ok(())
    } else {
        Err(ApiError::NoCapability(format!(
            "this needs {} on {}",
            capability_word(capability),
            filter_words(filter)
        )))
    }
}

/// Refuses a caller who holds `capability` on no filter at all.
///
/// # Errors
///
/// [`ApiError::NoCapability`].
pub fn require_any(
    state: &State,
    person: &TagValue,
    capability: Capability,
) -> Result<(), ApiError> {
    if filters_with(state, person, capability).is_empty() && capability != Capability::Record {
        return Err(ApiError::NoCapability(format!(
            "this needs {} on some filter",
            capability_word(capability)
        )));
    }
    Ok(())
}

/// Whether the person is an org manager: adjust on the empty filter.
#[must_use]
pub fn is_org_manager(state: &State, person: &TagValue) -> bool {
    state.can(person, Capability::Adjust, &Filter::default())
}

/// Refuses everyone but an org manager.
///
/// # Errors
///
/// [`ApiError::NoCapability`].
pub fn require_org_manager(state: &State, person: &TagValue) -> Result<(), ApiError> {
    if is_org_manager(state, person) {
        Ok(())
    } else {
        Err(ApiError::NoCapability(
            "this needs an org manager".to_owned(),
        ))
    }
}

/// The filters the person holds `capability` on, as granted.
#[must_use]
pub fn filters_with(state: &State, person: &TagValue, capability: Capability) -> Vec<Filter> {
    state
        .grants()
        .iter()
        .filter(|grant| grant.person == *person && grant.capability == capability)
        .map(|grant| grant.filter.clone())
        .collect()
}

/// The filters the person may view.
#[must_use]
pub fn view_filters(state: &State, person: &TagValue) -> Vec<Filter> {
    filters_with(state, person, Capability::View)
}

/// Whether a closed tag set lies inside one of the person's grants for
/// `capability`.
#[must_use]
pub fn covers(state: &State, person: &TagValue, capability: Capability, tags: &Tags) -> bool {
    filters_with(state, person, capability)
        .iter()
        .any(|filter| filter.matches(tags))
}

/// Whether the person may view a closed tag set.
#[must_use]
pub fn can_view(state: &State, person: &TagValue, tags: &Tags) -> bool {
    covers(state, person, Capability::View, tags)
}

/// The moment a new event is dated: `dated` when the caller gave one,
/// otherwise now. A date before the cutoff needs adjust.
///
/// # Errors
///
/// [`ApiError::NoCapability`] when the date is before the cutoff and the
/// caller has no adjust, or [`ApiError::BadRequest`] for a date in the
/// future.
pub fn dated(
    state: &State,
    person: &TagValue,
    dated: Option<OffsetDateTime>,
) -> Result<OffsetDateTime, ApiError> {
    let now = OffsetDateTime::now_utc();
    let Some(moment) = dated else {
        return Ok(now);
    };
    if moment > now {
        return Err(ApiError::BadRequest(
            "an event cannot be dated in the future".to_owned(),
        ));
    }
    if state
        .cutoff()
        .is_some_and(|cutoff| date_of(moment) < cutoff)
    {
        require(state, person, Capability::Adjust, &Filter::default()).map_err(|_| {
            ApiError::NoCapability(
                "the date is before the cutoff; this needs adjust, which org managers hold"
                    .to_owned(),
            )
        })?;
    }
    Ok(moment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use tokencontroller_core::{Currency, Event, Grant, Record, Role, TagName};

    fn person(text: &str) -> TagValue {
        TagValue::new(text).unwrap()
    }

    fn filter(pairs: &[&str]) -> Filter {
        Filter(Tags::parse(pairs.iter().copied()).unwrap())
    }

    fn state(grants: Vec<Grant>, cutoff: Option<time::Date>) -> State {
        let mut events: Vec<Event> = grants
            .into_iter()
            .enumerate()
            .map(|(index, grant)| {
                Event::new(
                    &format!("g{index}"),
                    datetime!(2026-01-01 00:00 UTC),
                    Record::Grant(grant),
                )
                .unwrap()
            })
            .collect();
        if let Some(cutoff) = cutoff {
            events.push(
                Event::new(
                    "cutoff",
                    datetime!(2026-01-02 00:00 UTC),
                    Record::Cutoff(cutoff),
                )
                .unwrap(),
            );
        }
        State::fold(&events, datetime!(2026-12-31 00:00 UTC), Currency::USD).unwrap()
    }

    #[test]
    fn a_people_manager_acts_inside_their_filter_only() {
        let web = filter(&["team=web"]);
        let state = state(
            Role::PeopleManager.grants(&person("priya"), std::slice::from_ref(&web)),
            None,
        );
        let priya = person("priya");
        assert!(require(&state, &priya, Capability::View, &web).is_ok());
        assert!(
            require(
                &state,
                &priya,
                Capability::View,
                &filter(&["team=web", "project=shop"])
            )
            .is_ok()
        );
        let refused = require(&state, &priya, Capability::View, &Filter::default()).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "this needs view on the whole organization"
        );
        assert!(require(&state, &priya, Capability::Record, &Filter::default()).is_ok());
        assert!(require_any(&state, &priya, Capability::Approve).is_ok());
        assert!(require_any(&state, &priya, Capability::Adjust).is_err());
        assert!(require_any(&state, &priya, Capability::Record).is_ok());
        assert!(!is_org_manager(&state, &priya));
        assert!(require_org_manager(&state, &priya).is_err());
        assert_eq!(view_filters(&state, &priya), vec![web.clone()]);
        let mut tags = web.0.clone();
        tags.insert(TagName::project(), person("shop"));
        assert!(can_view(&state, &priya, &tags));
        assert!(!can_view(&state, &priya, &Tags::new()));
        assert!(covers(&state, &priya, Capability::Assign, &tags));
        assert_eq!(filter_words(&web), "team web");
        assert_eq!(filter_words(&Filter::default()), "the whole organization");
        assert_eq!(capability_word(Capability::Cutoff), "cutoff");
    }

    #[test]
    fn an_org_manager_holds_everything() {
        let state = state(Role::OrgManager.grants(&person("nadia"), &[]), None);
        let nadia = person("nadia");
        assert!(is_org_manager(&state, &nadia));
        assert!(require_org_manager(&state, &nadia).is_ok());
        assert!(require(&state, &nadia, Capability::Plan, &filter(&["client=acme"])).is_ok());
        assert!(can_view(&state, &nadia, &Tags::new()));
    }

    #[test]
    fn dating_an_event_before_the_cutoff_needs_adjust() {
        let cutoff = Some(date!(2026 - 03 - 31));
        let managers = state(
            Role::OrgManager
                .grants(&person("nadia"), &[])
                .into_iter()
                .chain(Role::AgentManager.grants(&person("mara"), &[]))
                .collect(),
            cutoff,
        );
        let before = datetime!(2026-03-15 10:00 UTC);
        assert_eq!(
            dated(&managers, &person("nadia"), Some(before)).unwrap(),
            before
        );
        let refused = dated(&managers, &person("mara"), Some(before)).unwrap_err();
        assert!(refused.to_string().contains("adjust"));
        let after = datetime!(2026-04-15 10:00 UTC);
        assert_eq!(
            dated(&managers, &person("mara"), Some(after)).unwrap(),
            after
        );
        assert!(dated(&managers, &person("mara"), None).unwrap() <= OffsetDateTime::now_utc());
        let future = OffsetDateTime::now_utc() + time::Duration::days(2);
        assert!(dated(&managers, &person("nadia"), Some(future)).is_err());
    }
}
