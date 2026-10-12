//! What every request handler and background job can reach.

use std::sync::Arc;

use sqlx::PgPool;
use tokencontroller_core::Extension;

use crate::Config;
use crate::hooks::{NoHooks, ServiceHooks};

/// Shared state, cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub extension: Arc<dyn Extension>,
    /// The plug-in points of a build that adds its own areas; [`NoHooks`]
    /// in the open build.
    pub hooks: Arc<dyn ServiceHooks>,
}

impl AppState {
    #[must_use]
    pub fn new(db: PgPool, config: Config, extension: Arc<dyn Extension>) -> Self {
        Self {
            db,
            config: Arc::new(config),
            extension,
            hooks: Arc::new(NoHooks),
        }
    }

    /// The same state with `hooks` in place of the current ones.
    #[must_use]
    pub fn with_hooks(mut self, hooks: Arc<dyn ServiceHooks>) -> Self {
        self.hooks = hooks;
        self
    }
}
