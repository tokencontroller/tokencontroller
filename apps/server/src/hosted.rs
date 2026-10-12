//! The hosted service's own areas: Team plan billing ([`billing`]) and
//! customer relations ([`customer_relations`]). A self-hosted instance has
//! no use for them, so they reach the service only through its plug-in
//! points ([`crate::hooks`] and [`crate::serve_with_additions`]), with their
//! own settings and state, and can live outside this repo.
//!
//! [`billing`]: crate::billing
//! [`customer_relations`]: crate::customer_relations

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use axum::extract::FromRef;
use serde_json::Value;
use tokencontroller_core::Extension;
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use crate::billing::{self, StripeConfig};
use crate::customer_relations;
use crate::hooks::{HookFuture, MemberLimit, ServiceHooks};
use crate::sign_in::Caller;
use crate::{AppState, jobs};

/// Why the hosted service refuses one of its settings.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    CustomerRelations(#[from] customer_relations::SettingsError),
    #[error(
        "TC_STRIPE_SECRET_KEY and TC_STRIPE_WEBHOOK_SECRET go together: set both for Team plan billing, or neither"
    )]
    StripeHalfSet,
}

/// The hosted service's settings, on top of the service's own.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Settings {
    /// The support inbox, site assistant and newsletter settings, all off
    /// unless set.
    pub customer_relations: customer_relations::Settings,
    /// Team plan billing through Stripe. `None` keeps billing off.
    pub stripe: Option<StripeConfig>,
}

impl Settings {
    /// Reads the settings from the process environment.
    ///
    /// # Errors
    ///
    /// A [`SettingsError`] naming the variable to fix.
    pub fn from_env() -> Result<Self, SettingsError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Reads the settings through `lookup`, which returns a variable's value
    /// or `None` when it is not set.
    ///
    /// # Errors
    ///
    /// A [`SettingsError`] naming the variable to fix.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, SettingsError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let read = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
        let customer_relations = customer_relations::Settings::from_lookup(&lookup)?;
        let stripe = match (
            read("TC_STRIPE_SECRET_KEY"),
            read("TC_STRIPE_WEBHOOK_SECRET"),
        ) {
            (Some(secret_key), Some(webhook_secret)) => Some(StripeConfig {
                secret_key,
                webhook_secret,
            }),
            (None, None) => None,
            _ => return Err(SettingsError::StripeHalfSet),
        };
        Ok(Self {
            customer_relations,
            stripe,
        })
    }
}

/// Never prints secrets, so the settings can be logged.
impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("customer_relations", &self.customer_relations)
            .field("stripe", &self.stripe.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// What the hosted areas' handlers and background work reach: the service's
/// shared state, through `Deref`, and the hosted settings.
#[derive(Clone)]
pub struct Hosted {
    pub app: AppState,
    pub settings: Arc<Settings>,
}

impl Hosted {
    /// The hosted state on top of `app`, with the hosted [`Hooks`] put into
    /// `app`, so the service's own areas see the plans too.
    #[must_use]
    pub fn new(app: AppState, settings: Arc<Settings>) -> Self {
        let app = app.with_hooks(Arc::new(Hooks::new(Arc::clone(&settings))));
        Self { app, settings }
    }
}

impl Deref for Hosted {
    type Target = AppState;

    fn deref(&self) -> &AppState {
        &self.app
    }
}

impl FromRef<Hosted> for AppState {
    fn from_ref(hosted: &Hosted) -> Self {
        hosted.app.clone()
    }
}

/// The hosted service's answers to the service's plug-in points: the Solo
/// and Team plan member limits, and the operators' MCP tools.
pub struct Hooks {
    settings: Arc<Settings>,
}

impl Hooks {
    #[must_use]
    pub fn new(settings: Arc<Settings>) -> Self {
        Self { settings }
    }

    fn hosted(&self, state: &AppState) -> Hosted {
        Hosted {
            app: state.clone(),
            settings: Arc::clone(&self.settings),
        }
    }
}

impl ServiceHooks for Hooks {
    fn member_limit<'a>(
        &'a self,
        state: &'a AppState,
        organization_id: Uuid,
    ) -> HookFuture<'a, Option<MemberLimit>> {
        Box::pin(async move { billing::member_limit(&self.hosted(state), organization_id).await })
    }

    fn mcp_tools<'a>(
        &'a self,
        state: &'a AppState,
        caller: &'a Caller,
    ) -> HookFuture<'a, Vec<Value>> {
        Box::pin(async move {
            Ok(
                if customer_relations::is_operator(&self.hosted(state), caller.person_id).await? {
                    customer_relations::operator_tools()
                } else {
                    Vec::new()
                },
            )
        })
    }

    fn call_mcp_tool<'a>(
        &'a self,
        state: &'a AppState,
        caller: &'a Caller,
        name: &'a str,
        arguments: &'a Value,
    ) -> HookFuture<'a, Option<Value>> {
        Box::pin(async move {
            let hosted = self.hosted(state);
            if customer_relations::is_operator(&hosted, caller.person_id).await? {
                customer_relations::call_operator_tool(&hosted, name, arguments).await
            } else {
                Ok(None)
            }
        })
    }
}

/// The hosted areas' routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<Hosted> {
    OpenApiRouter::new()
        .merge(billing::routes())
        .merge(customer_relations::routes())
}

/// The hosted areas' routes with their state, for
/// [`crate::serve_with_additions`] and [`crate::router_with`].
#[must_use]
pub fn api_routes(hosted: Hosted) -> OpenApiRouter<AppState> {
    routes().with_state(hosted)
}

/// The OpenAPI document of the hosted areas' routes, for
/// [`crate::openapi_json_with`].
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    OpenApiRouter::<Hosted>::new()
        .nest(crate::api::API_PREFIX, routes())
        .into_openapi()
}

/// The whole HTTP service with the hosted areas, as [`crate::router`].
pub fn router(hosted: Hosted) -> axum::Router {
    crate::router_with(hosted.app.clone(), api_routes(hosted))
}

/// Starts the hosted areas' background work: the customer relations loop
/// and the `onboarding-emails` job when it is on.
pub fn start(hosted: &Hosted) {
    customer_relations::start(hosted.clone());
    if hosted.settings.customer_relations.onboarding_emails {
        let job = customer_relations::onboarding_job(Arc::clone(&hosted.settings));
        // Dropping the handle leaves the task running.
        drop(jobs::spawn(hosted.app.clone(), vec![job]));
    }
}

/// Runs the service with the hosted areas: reads the hosted settings, then
/// serves as [`crate::serve`] does.
///
/// # Errors
///
/// As [`crate::serve`], or a [`SettingsError`].
pub async fn serve(extension: Arc<dyn Extension>) -> anyhow::Result<()> {
    let settings = Arc::new(Settings::from_env()?);
    let hooks = Arc::new(Hooks::new(Arc::clone(&settings)));
    crate::serve_with_additions(extension, hooks, move |app| async move {
        let hosted = Hosted::new(app, settings);
        start(&hosted);
        Ok(crate::Additions {
            api: api_routes(hosted),
            routes: axum::Router::new(),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn settings(pairs: &[(&str, &str)]) -> Result<Settings, SettingsError> {
        let vars: HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        Settings::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn stripe_needs_both_settings() {
        assert_eq!(settings(&[]).unwrap().stripe, None);
        let mut pairs = vec![("TC_STRIPE_SECRET_KEY", "sk_test_123")];
        assert_eq!(settings(&pairs), Err(SettingsError::StripeHalfSet));

        pairs.push(("TC_STRIPE_WEBHOOK_SECRET", "whsec_456"));
        let stripe = settings(&pairs).unwrap().stripe.unwrap();
        assert_eq!(stripe.secret_key, "sk_test_123");
        assert_eq!(stripe.webhook_secret, "whsec_456");
    }

    #[test]
    fn debug_output_hides_the_stripe_keys() {
        let pairs = [
            ("TC_STRIPE_SECRET_KEY", "sk_test_hidden"),
            ("TC_STRIPE_WEBHOOK_SECRET", "whsec_hidden"),
        ];
        let shown = format!("{:?}", settings(&pairs).unwrap());
        assert!(!shown.contains("sk_test_hidden"));
        assert!(!shown.contains("whsec_hidden"));
    }
}
