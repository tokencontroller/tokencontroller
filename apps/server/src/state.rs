//! What every request handler and background job can reach.

use std::sync::Arc;

use sqlx::PgPool;
use tokencontroller_core::Extension;

use crate::Config;

/// Shared state, cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub extension: Arc<dyn Extension>,
}

impl AppState {
    #[must_use]
    pub fn new(db: PgPool, config: Config, extension: Arc<dyn Extension>) -> Self {
        Self {
            db,
            config: Arc::new(config),
            extension,
        }
    }
}
