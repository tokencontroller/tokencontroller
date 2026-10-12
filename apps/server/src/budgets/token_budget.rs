//! Token budgets: money a person planned for a tag filter and a period
//! (docs: Account > Token budgets and forecasts). People managers set them
//! within the filters they are assigned, controllers anywhere. Token budgets
//! can overlap, so two of them can count the same money: they are never
//! added up. A token budget never stops an agent and sends no alert.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::inherited::Inherited;
use crate::AppState;
use crate::bills::amount::{format_amount, parse_amount};
use crate::close::period::Period;
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::csv::Csv;
use crate::reports::filter::Filter;
use crate::reports::scope::Scope;
use crate::sign_in::{Caller, Credential};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_token_budgets, create_token_budget))
        .routes(routes!(update_token_budget, remove_token_budget))
}

/// When a token budget applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// One period.
    Once(Period),
    /// Every period from this one on, the amount each month.
    EveryMonth(Period),
    /// All time, as for a ticket.
    AllTime,
}

impl Applies {
    /// Reads the stored period and monthly flag.
    ///
    /// # Errors
    ///
    /// A message when `every_month` has no period to start from.
    pub fn new(period: Option<Period>, every_month: bool) -> Result<Self, String> {
        match (period, every_month) {
            (Some(period), false) => Ok(Self::Once(period)),
            (Some(period), true) => Ok(Self::EveryMonth(period)),
            (None, false) => Ok(Self::AllTime),
            (None, true) => Err(
                "a token budget for every month needs the period it starts in, such as 2026-09"
                    .to_owned(),
            ),
        }
    }

    /// Whether the token budget is in force in `period`.
    #[must_use]
    pub fn in_force(self, period: Period) -> bool {
        match self {
            Self::Once(once) => once == period,
            Self::EveryMonth(from) => from <= period,
            Self::AllTime => true,
        }
    }

    /// The period actual is counted over when `period` is shown: that period,
    /// or `None` for all time.
    #[must_use]
    pub const fn actual_period(self, period: Period) -> Option<Period> {
        match self {
            Self::Once(_) | Self::EveryMonth(_) => Some(period),
            Self::AllTime => None,
        }
    }

    /// The period stored: the one month, or the first of every month.
    #[must_use]
    pub const fn period(self) -> Option<Period> {
        match self {
            Self::Once(period) | Self::EveryMonth(period) => Some(period),
            Self::AllTime => None,
        }
    }

    #[must_use]
    pub const fn every_month(self) -> bool {
        matches!(self, Self::EveryMonth(_))
    }

    /// In words: `2026-09`, `every month from 2026-09` or `all time`.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Once(period) => period.to_string(),
            Self::EveryMonth(period) => format!("every month from {period}"),
            Self::AllTime => "all time".to_owned(),
        }
    }
}

/// One token budget as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBudget {
    pub id: Uuid,
    pub name: Option<String>,
    pub filter: Filter,
    pub applies: Applies,
    pub amount: Money,
    pub token_forecast_id: Option<Uuid>,
    pub set_by: Uuid,
    pub set_by_name: Option<String>,
    pub set_at: OffsetDateTime,
}

/// The token budgets of an organization that are not removed, newest first;
/// with `id`, only that one.
///
/// # Errors
///
/// A database error.
pub async fn load(
    db: &PgPool,
    organization_id: Uuid,
    id: Option<Uuid>,
) -> Result<Vec<TokenBudget>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT b.id, b.name, b.filter, b.period, b.every_month, b.amount_nanos, b.currency,
                  b.token_forecast_id, b.set_by, b.set_at, p.name AS "set_by_name?"
           FROM token_budgets b LEFT JOIN people p ON p.id = b.set_by
           WHERE b.organization_id = $1 AND b.removed_at IS NULL AND ($2::uuid IS NULL OR b.id = $2)
           ORDER BY b.set_at DESC, b.id"#,
        organization_id,
        id,
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let currency = Currency::new(&row.currency).ok()?;
            let applies = Applies::new(row.period.map(Period::of_date), row.every_month).ok()?;
            Some(TokenBudget {
                id: row.id,
                name: row.name,
                filter: Filter::from_json(&row.filter),
                applies,
                amount: Money::from_nanos(row.amount_nanos, currency),
                token_forecast_id: row.token_forecast_id,
                set_by: row.set_by,
                set_by_name: row.set_by_name,
                set_at: row.set_at,
            })
        })
        .collect())
}

/// What a caller may see and set.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub scope: Scope,
    /// The caller's email: an agent manager sees the token budgets on
    /// `person: <their email>`.
    pub email: Option<String>,
    pub role: Role,
}

impl Viewer {
    /// The viewer `caller` is.
    ///
    /// # Errors
    ///
    /// A database error.
    pub async fn of(db: &PgPool, caller: &Caller) -> Result<Self, sqlx::Error> {
        let scope = Scope::of(db, caller).await?;
        let email = sqlx::query_scalar!("SELECT email FROM people WHERE id = $1", caller.person_id)
            .fetch_optional(db)
            .await?;
        Ok(Self {
            scope,
            email,
            role: caller.role,
        })
    }

    /// Whether the viewer sees a token budget on `filter`, which `expanded`
    /// is with its inherited tags: a controller all of them, a people
    /// manager those within their filters, and everyone those on their own
    /// person.
    #[must_use]
    pub fn sees(&self, filter: &Filter, expanded: &Filter) -> bool {
        self.scope.is_everything()
            || (self.email.is_some() && filter.0.get("person") == self.email.as_ref())
            || self.may_set(expanded)
    }

    /// Whether the viewer may set a token budget on a filter, given with its
    /// inherited tags: a controller anywhere, a people manager within their
    /// filters.
    #[must_use]
    pub fn may_set(&self, expanded: &Filter) -> bool {
        self.role >= Role::PeopleManager && self.scope.covers(expanded)
    }
}

/// The token budgets `viewer` sees.
///
/// # Errors
///
/// A database error.
pub async fn visible(
    db: &PgPool,
    organization_id: Uuid,
    viewer: &Viewer,
) -> Result<Vec<TokenBudget>, sqlx::Error> {
    let budgets = load(db, organization_id, None).await?;
    let filters: Vec<&Filter> = budgets.iter().map(|budget| &budget.filter).collect();
    let inherited = Inherited::load(db, organization_id, &filters).await?;
    Ok(budgets
        .into_iter()
        .filter(|budget| viewer.sees(&budget.filter, &inherited.expand(&budget.filter)))
        .collect())
}

/// One token budget the viewer sees, or [`ApiError::NotFound`].
pub(super) async fn find_visible(
    db: &PgPool,
    organization_id: Uuid,
    viewer: &Viewer,
    id: Uuid,
) -> Result<TokenBudget, ApiError> {
    let budget = load(db, organization_id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)?;
    let inherited = Inherited::load(db, organization_id, &[&budget.filter]).await?;
    if viewer.sees(&budget.filter, &inherited.expand(&budget.filter)) {
        Ok(budget)
    } else {
        Err(ApiError::NotFound)
    }
}

/// One token budget as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenBudgetView {
    pub id: Uuid,
    pub name: Option<String>,
    /// The tag filter, such as `{"person": "ana@example.com"}`.
    pub filter: Filter,
    /// The one period, or the first period of a token budget for every
    /// month; `null` for all time.
    #[schema(example = "2026-09")]
    pub period: Option<String>,
    /// The amount applies to each month from `period` on.
    pub every_month: bool,
    /// When it applies, in words: `2026-09`, `every month from 2026-09` or
    /// `all time`.
    #[schema(example = "every month from 2026-09")]
    pub applies: String,
    /// The token budget, such as `1500.00`.
    #[schema(example = "1500.00")]
    pub amount: String,
    #[schema(example = "EUR")]
    pub currency: String,
    /// The token forecast it was accepted from.
    pub token_forecast_id: Option<Uuid>,
    /// Who set it.
    pub set_by: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub set_at: OffsetDateTime,
}

impl From<&TokenBudget> for TokenBudgetView {
    fn from(budget: &TokenBudget) -> Self {
        Self {
            id: budget.id,
            name: budget.name.clone(),
            filter: budget.filter.clone(),
            period: budget.applies.period().map(|period| period.to_string()),
            every_month: budget.applies.every_month(),
            applies: budget.applies.describe(),
            amount: format_amount(budget.amount),
            currency: budget.amount.currency().code(),
            token_forecast_id: budget.token_forecast_id,
            set_by: budget.set_by_name.clone(),
            set_at: budget.set_at,
        }
    }
}

/// A token budget to set.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TokenBudgetInput {
    /// A name to tell it apart, such as `Ana, monthly`.
    pub name: Option<String>,
    /// The tag filter: tag names and values, such as `{"team": "web",
    /// "client": "acme"}`. No tags is the whole organization.
    #[serde(default)]
    pub filter: Filter,
    /// The period, such as `2026-09`. Left out: all time, as for a ticket.
    #[schema(example = "2026-09")]
    pub period: Option<String>,
    /// The amount applies to every month from `period` on.
    #[serde(default)]
    pub every_month: bool,
    /// The amount in the reporting currency, above zero, such as `1500.00`.
    #[schema(example = "1500.00")]
    pub amount: String,
}

/// A token budget input after checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub name: Option<String>,
    pub filter: Filter,
    pub applies: Applies,
    pub amount: Money,
}

impl TokenBudgetInput {
    /// Checks the input, with the amount read in `currency`.
    ///
    /// # Errors
    ///
    /// A plain message saying what to fix.
    pub fn check(self, currency: Currency) -> Result<Draft, String> {
        let mut filter = Filter::default();
        for (name, value) in self.filter.0 {
            let (name, value) = (name.trim().to_owned(), value.trim().to_owned());
            if name.is_empty() || value.is_empty() {
                return Err(
                    "every tag of the filter needs a name and a value, such as team: web"
                        .to_owned(),
                );
            }
            if filter.0.insert(name.clone(), value).is_some() {
                return Err(format!(
                    "the filter names {name:?} twice; a filter has one value per tag name"
                ));
            }
        }
        let period = self
            .period
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::parse::<Period>)
            .transpose()
            .map_err(|error| error.to_string())?;
        let applies = Applies::new(period, self.every_month)?;
        let amount = parse_amount(&self.amount, currency).map_err(|error| error.to_string())?;
        if amount.nanos() <= 0 {
            return Err("a token budget needs an amount above zero".to_owned());
        }
        let name = self
            .name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        Ok(Draft {
            name,
            filter,
            applies,
            amount,
        })
    }
}

/// Stores a new token budget and returns its ID.
///
/// # Errors
///
/// A database error.
pub async fn insert(
    db: &PgPool,
    organization_id: Uuid,
    draft: &Draft,
    token_forecast_id: Option<Uuid>,
    set_by: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"INSERT INTO token_budgets
               (organization_id, name, filter, period, every_month, amount_nanos, currency,
                token_forecast_id, set_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           RETURNING id"#,
        organization_id,
        draft.name,
        draft.filter.to_json(),
        draft.applies.period().map(Period::first_day),
        draft.applies.every_month(),
        draft.amount.nanos(),
        draft.amount.currency().code(),
        token_forecast_id,
        set_by,
    )
    .fetch_one(db)
    .await
}

/// One stored token budget as the API shows it.
///
/// # Errors
///
/// [`ApiError::NotFound`] when it is gone, or a database error.
pub async fn view(
    db: &PgPool,
    organization_id: Uuid,
    id: Uuid,
) -> Result<TokenBudgetView, ApiError> {
    load(db, organization_id, Some(id))
        .await?
        .first()
        .map(TokenBudgetView::from)
        .ok_or(ApiError::NotFound)
}

async fn reporting_currency(db: &PgPool, organization_id: Uuid) -> Result<Currency, ApiError> {
    crate::bills::reporting_currency(db, organization_id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Checks that `viewer` may set a token budget on `filter`.
async fn require_may_set(
    db: &PgPool,
    organization_id: Uuid,
    viewer: &Viewer,
    filter: &Filter,
) -> Result<(), ApiError> {
    let inherited = Inherited::load(db, organization_id, &[filter]).await?;
    if viewer.may_set(&inherited.expand(filter)) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// How to show the list.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// `csv` for a CSV file.
    pub format: Option<String>,
}

/// The token budgets as a CSV file.
#[must_use]
pub fn to_csv(views: &[TokenBudgetView]) -> Csv {
    let mut csv = Csv::new([
        "token_budget",
        "name",
        "filter",
        "applies",
        "amount",
        "currency",
        "token_forecast",
        "set_by",
        "set_at",
    ]);
    for view in views {
        csv.row([
            view.id.to_string(),
            view.name.clone().unwrap_or_default(),
            view.filter.to_string(),
            view.applies.clone(),
            view.amount.clone(),
            view.currency.clone(),
            view.token_forecast_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            view.set_by.clone().unwrap_or_default(),
            view.set_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
        ]);
    }
    csv
}

/// The token budgets you may see: a controller all of them, a people
/// manager those within their filters, an agent manager those on their own
/// person. They can overlap; never add them up.
#[utoipa::path(
    get,
    path = "/token-budgets",
    tag = "token_budgets",
    params(ListQuery),
    responses(
        (status = OK, description = "The token budgets, as JSON or CSV", content((Vec<TokenBudgetView> = "application/json"), (String = "text/csv"))),
    ),
)]
pub(crate) async fn list_token_budgets(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<ListQuery>,
) -> Result<Response, ApiError> {
    let viewer = Viewer::of(&state.db, &caller).await?;
    let views: Vec<TokenBudgetView> = visible(&state.db, caller.organization_id, &viewer)
        .await?
        .iter()
        .map(TokenBudgetView::from)
        .collect();
    if query.format.as_deref() == Some("csv") {
        return Ok(to_csv(&views).into_download("token-budgets.csv"));
    }
    Ok(Json(views).into_response())
}

/// Set a token budget on a tag filter: for one period, every month from a
/// period on, or all time. People managers set them within the filters they
/// are assigned, controllers anywhere.
#[utoipa::path(
    post,
    path = "/token-budgets",
    tag = "token_budgets",
    request_body = TokenBudgetInput,
    responses(
        (status = CREATED, description = "Set", body = TokenBudgetView),
        (status = BAD_REQUEST, description = "The filter, period or amount is not valid", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn create_token_budget(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<TokenBudgetInput>,
) -> Result<(StatusCode, Json<TokenBudgetView>), ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let organization_id = caller.organization_id;
    let currency = reporting_currency(&state.db, organization_id).await?;
    let draft = input.check(currency).map_err(ApiError::BadRequest)?;
    let viewer = Viewer::of(&state.db, &caller).await?;
    require_may_set(&state.db, organization_id, &viewer, &draft.filter).await?;
    let id = insert(&state.db, organization_id, &draft, None, caller.person_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(view(&state.db, organization_id, id).await?),
    ))
}

/// Change a token budget. Both its old and its new filter must lie within
/// the filters you are assigned.
#[utoipa::path(
    put,
    path = "/token-budgets/{id}",
    tag = "token_budgets",
    params(("id" = Uuid, Path, description = "The token budget")),
    request_body = TokenBudgetInput,
    responses(
        (status = OK, description = "Changed", body = TokenBudgetView),
        (status = BAD_REQUEST, description = "The filter, period or amount is not valid", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token budget", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn update_token_budget(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<TokenBudgetInput>,
) -> Result<Json<TokenBudgetView>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let organization_id = caller.organization_id;
    let viewer = Viewer::of(&state.db, &caller).await?;
    let budget = find_visible(&state.db, organization_id, &viewer, id).await?;
    require_may_set(&state.db, organization_id, &viewer, &budget.filter).await?;
    let draft = input
        .check(budget.amount.currency())
        .map_err(ApiError::BadRequest)?;
    require_may_set(&state.db, organization_id, &viewer, &draft.filter).await?;
    sqlx::query!(
        r#"UPDATE token_budgets
           SET name = $3, filter = $4, period = $5, every_month = $6, amount_nanos = $7,
               set_by = $8, set_at = now()
           WHERE id = $1 AND organization_id = $2 AND removed_at IS NULL"#,
        id,
        organization_id,
        draft.name,
        draft.filter.to_json(),
        draft.applies.period().map(Period::first_day),
        draft.applies.every_month(),
        draft.amount.nanos(),
        caller.person_id,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(view(&state.db, organization_id, id).await?))
}

/// Remove a token budget. It no longer shows in token budget vs actual.
#[utoipa::path(
    delete,
    path = "/token-budgets/{id}",
    tag = "token_budgets",
    params(("id" = Uuid, Path, description = "The token budget")),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token budget", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn remove_token_budget(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let organization_id = caller.organization_id;
    let viewer = Viewer::of(&state.db, &caller).await?;
    let budget = find_visible(&state.db, organization_id, &viewer, id).await?;
    require_may_set(&state.db, organization_id, &viewer, &budget.filter).await?;
    sqlx::query!(
        "UPDATE token_budgets SET removed_at = now() WHERE id = $1 AND organization_id = $2 AND removed_at IS NULL",
        id,
        organization_id,
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn period(text: &str) -> Period {
        text.parse().unwrap()
    }

    fn input(
        filter: &str,
        period: Option<&str>,
        every_month: bool,
        amount: &str,
    ) -> TokenBudgetInput {
        TokenBudgetInput {
            name: Some("  Ana, monthly ".to_owned()),
            filter: Filter::parse(filter).unwrap(),
            period: period.map(str::to_owned),
            every_month,
            amount: amount.to_owned(),
        }
    }

    #[test]
    fn one_period_every_month_or_all_time() {
        let september = period("2026-09");
        let once = Applies::new(Some(september), false).unwrap();
        assert!(once.in_force(september));
        assert!(!once.in_force(period("2026-10")));
        assert!(!once.in_force(period("2026-08")));
        assert_eq!(once.actual_period(september), Some(september));
        assert_eq!(once.describe(), "2026-09");

        let monthly = Applies::new(Some(september), true).unwrap();
        assert!(monthly.in_force(september));
        assert!(monthly.in_force(period("2027-01")));
        assert!(!monthly.in_force(period("2026-08")));
        assert_eq!(
            monthly.actual_period(period("2026-11")),
            Some(period("2026-11"))
        );
        assert_eq!(monthly.describe(), "every month from 2026-09");
        assert!(monthly.every_month());
        assert_eq!(monthly.period(), Some(september));

        let all_time = Applies::new(None, false).unwrap();
        assert!(all_time.in_force(period("2020-01")));
        assert_eq!(all_time.actual_period(september), None);
        assert_eq!(all_time.describe(), "all time");
        assert_eq!(all_time.period(), None);
        assert!(!all_time.every_month());

        assert!(Applies::new(None, true).is_err());
    }

    #[test]
    fn checks_the_input() {
        let draft = input("person:ana@example.com", Some("2026-09"), true, "1500")
            .check(Currency::EUR)
            .unwrap();
        assert_eq!(draft.name.as_deref(), Some("Ana, monthly"));
        assert_eq!(draft.applies, Applies::EveryMonth(period("2026-09")));
        assert_eq!(
            draft.amount,
            Money::from_minor_units(150_000, Currency::EUR).unwrap()
        );
        assert_eq!(draft.filter.to_string(), "person:ana@example.com");

        let all_time = input("ticket:KD-214", Some(" "), false, "0.01")
            .check(Currency::EUR)
            .unwrap();
        assert_eq!(all_time.applies, Applies::AllTime);
    }

    #[test]
    fn refuses_what_is_not_a_token_budget() {
        for amount in ["0", "-5", "0.00", "abc", ""] {
            assert!(
                input("team:web", None, false, amount)
                    .check(Currency::EUR)
                    .is_err(),
                "{amount}"
            );
        }
        assert!(
            input("team:web", Some("September"), false, "10")
                .check(Currency::EUR)
                .is_err()
        );
        assert!(
            input("team:web", None, true, "10")
                .check(Currency::EUR)
                .is_err()
        );
        let mut blank = input("team:web", None, false, "10");
        blank.filter.0.insert("client".to_owned(), "  ".to_owned());
        assert!(blank.check(Currency::EUR).is_err());
        let mut twice = input("team:web", None, false, "10");
        twice
            .filter
            .0
            .insert(" team ".to_owned(), "data".to_owned());
        assert!(twice.check(Currency::EUR).is_err());
        let mut unnamed = input("", None, false, "10");
        unnamed.name = Some("   ".to_owned());
        assert_eq!(unnamed.check(Currency::EUR).unwrap().name, None);
    }

    fn viewer(role: Role, scope: Scope) -> Viewer {
        Viewer {
            scope,
            email: Some("ana@example.com".to_owned()),
            role,
        }
    }

    #[test]
    fn who_sees_and_sets_which_token_budget() {
        let mine = Filter::parse("person:ana@example.com").unwrap();
        let web = Filter::parse("team:web").unwrap();
        let web_acme = Filter::parse("team:web,client:acme").unwrap();
        let data = Filter::parse("team:data").unwrap();

        let agent = viewer(
            Role::AgentManager,
            Scope::of_person(Uuid::nil(), Vec::new()),
        );
        assert!(agent.sees(&mine, &mine));
        assert!(!agent.sees(&web, &web));
        assert!(!agent.may_set(&mine));

        let people = viewer(
            Role::PeopleManager,
            Scope::of_person(Uuid::nil(), vec![web.clone()]),
        );
        assert!(people.sees(&web_acme, &web_acme));
        assert!(people.may_set(&web_acme));
        assert!(people.may_set(&web));
        assert!(!people.sees(&data, &data));
        assert!(!people.may_set(&data));
        assert!(people.sees(&mine, &mine));
        assert!(!people.may_set(&mine));
        let bo = Filter::parse("person:bo@example.com").unwrap();
        let bo_in_web = Filter::parse("person:bo@example.com,team:web").unwrap();
        assert!(!people.sees(&bo, &bo));
        assert!(people.sees(&bo, &bo_in_web));
        assert!(people.may_set(&bo_in_web));

        let controller = viewer(Role::Controller, Scope::everything());
        assert!(controller.sees(&data, &data));
        assert!(controller.may_set(&Filter::default()));

        let nobody = Viewer {
            scope: Scope::of_person(Uuid::nil(), Vec::new()),
            email: None,
            role: Role::AgentManager,
        };
        assert!(!nobody.sees(&Filter::default(), &Filter::default()));
    }

    #[test]
    fn views_and_csv_show_the_token_budget() {
        let budget = TokenBudget {
            id: Uuid::nil(),
            name: Some("Ana".to_owned()),
            filter: Filter::parse("person:ana@example.com").unwrap(),
            applies: Applies::EveryMonth(period("2026-09")),
            amount: Money::from_minor_units(150_000, Currency::EUR).unwrap(),
            token_forecast_id: None,
            set_by: Uuid::nil(),
            set_by_name: Some("Jo".to_owned()),
            set_at: time::macros::datetime!(2026-09-01 09:00 UTC),
        };
        let view = TokenBudgetView::from(&budget);
        assert_eq!(view.amount, "1500.00");
        assert_eq!(view.currency, "EUR");
        assert_eq!(view.period.as_deref(), Some("2026-09"));
        assert_eq!(view.applies, "every month from 2026-09");
        let text = to_csv(&[view]).into_text();
        assert_eq!(
            text,
            "token_budget,name,filter,applies,amount,currency,token_forecast,set_by,set_at\r\n\
             00000000-0000-0000-0000-000000000000,Ana,person:ana@example.com,every month from 2026-09,1500.00,EUR,,Jo,2026-09-01T09:00:00Z\r\n"
        );
    }
}
