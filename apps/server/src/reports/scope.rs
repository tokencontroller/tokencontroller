//! Who sees which cost (docs: Account > Roles). An agent manager sees their
//! own token entries. A people manager also sees everything within the
//! filters they are assigned. A controller sees everything. A read token
//! sees what its creator's role sees.

use sqlx::PgPool;
use uuid::Uuid;

use super::filter::Filter;
use super::lines::CostLine;
use crate::organizations::Role;
use crate::sign_in::Caller;

/// What one caller may see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    everything: bool,
    own: Option<Uuid>,
    filters: Vec<Filter>,
}

impl Scope {
    /// Everything in the organization, for controllers and for work the
    /// service does on its own.
    #[must_use]
    pub const fn everything() -> Self {
        Self {
            everything: true,
            own: None,
            filters: Vec::new(),
        }
    }

    /// One person's own token entries plus everything within `filters`.
    #[must_use]
    pub const fn of_person(person_id: Uuid, filters: Vec<Filter>) -> Self {
        Self {
            everything: false,
            own: Some(person_id),
            filters,
        }
    }

    /// What `caller` may see.
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn of(db: &PgPool, caller: &Caller) -> Result<Self, sqlx::Error> {
        match caller.role {
            Role::Controller => Ok(Self::everything()),
            Role::PeopleManager => {
                let filters = crate::organizations::filters_of(db, caller.person_id)
                    .await?
                    .into_iter()
                    .map(Filter)
                    .collect();
                Ok(Self::of_person(caller.person_id, filters))
            }
            Role::AgentManager => Ok(Self::of_person(caller.person_id, Vec::new())),
        }
    }

    /// Whether the scope covers the whole organization.
    #[must_use]
    pub const fn is_everything(&self) -> bool {
        self.everything
    }

    /// The people manager's filters.
    #[must_use]
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// Whether a cost line is visible.
    #[must_use]
    pub fn sees(&self, line: &CostLine) -> bool {
        self.everything
            || (self.own.is_some() && self.own == line.agent_manager_id)
            || self.filters.iter().any(|filter| filter.matches(&line.tags))
    }

    /// Whether the scope may act on token entries matching `filter`, such as
    /// setting a token budget or a cadence on it: everything a controller
    /// sees, and for a people manager a filter within one of theirs.
    #[must_use]
    pub fn covers(&self, filter: &Filter) -> bool {
        self.everything || self.filters.iter().any(|mine| filter.lies_within(mine))
    }

    /// Whether the scope may approve or reject this cost line: not one's own
    /// unless a controller, and within one of the people manager's filters.
    #[must_use]
    pub fn may_decide(&self, line: &CostLine) -> bool {
        self.everything
            || (self.own != line.agent_manager_id
                && self.filters.iter().any(|filter| filter.matches(&line.tags)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::filter::Tags;
    use crate::reports::lines::EntryStatus;
    use time::macros::datetime;

    fn line(agent_manager: Uuid, tags: &[(&str, &str)]) -> CostLine {
        CostLine {
            split_id: Uuid::nil(),
            entry_id: Uuid::nil(),
            agent_manager_id: Some(agent_manager),
            provider: "anthropic".to_owned(),
            agent_harness: "claude-code".to_owned(),
            agent_name: None,
            model: "claude-opus-5".to_owned(),
            effort: None,
            speed: None,
            started_at: datetime!(2026-09-01 09:00 UTC),
            status: EntryStatus::Submitted,
            on_token_sheet: true,
            summary: String::new(),
            currency: tokencontroller_core::Currency::USD,
            list_value: None,
            billed_value: None,
            placed_by: "no ticket".to_owned(),
            ticket_not_synced: false,
            tags: tags
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect::<Tags>(),
        }
    }

    #[test]
    fn an_agent_manager_sees_their_own() {
        let me = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        let scope = Scope::of_person(me, Vec::new());
        assert!(scope.sees(&line(me, &[])));
        assert!(!scope.sees(&line(other, &[("team", "web")])));
        assert!(!scope.may_decide(&line(me, &[])));
        assert!(!scope.covers(&Filter::default()));
    }

    #[test]
    fn a_people_manager_sees_their_filters() {
        let me = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        let web = Filter::parse("team:web").unwrap();
        let scope = Scope::of_person(me, vec![web]);
        assert!(scope.sees(&line(other, &[("team", "web")])));
        assert!(!scope.sees(&line(other, &[("team", "data")])));
        assert!(scope.may_decide(&line(other, &[("team", "web")])));
        assert!(!scope.may_decide(&line(me, &[("team", "web")])));
        assert!(scope.covers(&Filter::parse("team:web,client:acme").unwrap()));
        assert!(!scope.covers(&Filter::parse("client:acme").unwrap()));
        assert!(!scope.covers(&Filter::default()));
    }

    #[test]
    fn a_controller_sees_everything() {
        let scope = Scope::everything();
        assert!(scope.is_everything());
        assert!(scope.sees(&line(Uuid::from_u128(9), &[])));
        assert!(scope.may_decide(&line(Uuid::from_u128(9), &[])));
        assert!(scope.covers(&Filter::default()));
    }
}
