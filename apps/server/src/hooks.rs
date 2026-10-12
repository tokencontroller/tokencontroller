//! The service's plug-in points for a build that adds areas of its own. They
//! sit next to the core crate's `Extension` (ADR 0001), which holds the hooks
//! that need no database; these get the shared state, so they can read and
//! write it.
//!
//! The open build uses [`NoHooks`]: every hook keeps the open behavior.

use std::future::Future;
use std::pin::Pin;

use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;

/// What a hook returns: a future the service awaits.
pub type HookFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ApiError>> + Send + 'a>>;

/// How many members an organization may have, and what to say when it is
/// reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberLimit {
    pub max: i64,
    pub message: String,
}

/// The plug-in points. Each has a default that keeps the open behavior.
pub trait ServiceHooks: Send + Sync + 'static {
    /// The member limit of one organization, or `None` to keep the
    /// instance's own (`Extension::member_limit`, else the Team plan's 150).
    fn member_limit<'a>(
        &'a self,
        state: &'a AppState,
        organization_id: Uuid,
    ) -> HookFuture<'a, Option<MemberLimit>> {
        let _ = (state, organization_id);
        Box::pin(async { Ok(None) })
    }
}

/// The hooks of the open build: every one keeps its default.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl ServiceHooks for NoHooks {}
