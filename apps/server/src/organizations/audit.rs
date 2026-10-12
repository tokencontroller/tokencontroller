//! The audit log hook. Every change the Enterprise plan's audit log records
//! goes through [`record_audit_event`]; the open edition keeps no audit log,
//! so there the call does nothing.

use tokencontroller_core::extension::AuditEvent;
use uuid::Uuid;

use crate::AppState;

/// Records one change: who did what to which record. `actor` is the member
/// who made it, or `None` for the service itself or an identity provider.
/// `action` is a dotted name such as `member.removed`.
pub fn record_audit_event(
    state: &AppState,
    organization_id: Uuid,
    actor: Option<Uuid>,
    action: &str,
    target: Option<Uuid>,
) {
    state.extension.record_audit_event(&AuditEvent {
        organization_id: organization_id.to_string(),
        actor_id: actor.map(|id| id.to_string()),
        action: action.to_owned(),
        target_id: target.map(|id| id.to_string()),
    });
}
