//! The one plug-in point of the open-source service (ADR 0001).
//!
//! The service calls an [`Extension`] at a small, fixed set of hooks. The
//! open-source binary uses [`NoExtension`], which does nothing, so the open
//! image carries no enterprise code. The Enterprise plan build links its own
//! implementation at build time as a normal Cargo dependency; nothing is
//! loaded at run time.
//!
//! Every hook has a default that keeps the open behavior, so adding a hook
//! never breaks an implementation. Add a hook only where the docs name an
//! Enterprise plan feature: sign-in providers, audit events, telemetry
//! forwarding, bill connectors and the notices that go with them.

/// Hooks the service calls. Implementations must be cheap and must not block:
/// a hook that needs the network spawns its own work.
pub trait Extension: Send + Sync + 'static {
    /// The edition name the service reports in its health response and logs.
    fn edition(&self) -> &'static str {
        "open"
    }

    /// Sign-in providers offered next to the magic link, such as an OIDC
    /// identity provider. The open edition offers none.
    fn sign_in_providers(&self) -> Vec<SignInProvider> {
        Vec::new()
    }

    /// Called once for every change the audit log would record. The open
    /// edition keeps no audit log.
    fn record_audit_event(&self, event: &AuditEvent) {
        let _ = event;
    }

    /// A message the web app shows to every controller of the organization,
    /// such as an Enterprise plan renewal notice. The open edition has none.
    fn controller_notice(&self) -> Option<String> {
        None
    }

    /// Called with every telemetry batch the service accepts, before prompt
    /// text is dropped, so it can be forwarded to the customer's own
    /// collector. The open edition forwards nothing.
    fn forward_telemetry(&self, batch: &TelemetryBatch<'_>) {
        let _ = batch;
    }
}

/// The extension of the open-source build: every hook keeps its default.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExtension;

impl Extension for NoExtension {}

/// A way to sign in besides the magic link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInProvider {
    /// Stable identifier used in the sign-in URL.
    pub id: String,
    /// The label on the sign-in button.
    pub label: String,
}

/// One change the audit log records: who did what to which record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub organization_id: String,
    /// The member who made the change, or `None` for the service itself.
    pub actor_id: Option<String>,
    /// What happened, such as `token_sheet.submitted`.
    pub action: String,
    /// The record it happened to, such as a token sheet ID.
    pub target_id: Option<String>,
}

/// A telemetry batch exactly as it arrived over OpenTelemetry.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryBatch<'a> {
    pub organization_id: &'a str,
    /// Which OTLP endpoint it arrived on, so a forwarder can send it to the
    /// same endpoint of another collector.
    pub signal: TelemetrySignal,
    /// `application/x-protobuf` or `application/json`.
    pub content_type: &'a str,
    pub body: &'a [u8],
}

/// The kind of OpenTelemetry data in a batch, one per OTLP endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetrySignal {
    /// Arrived on `/v1/metrics`.
    Metrics,
    /// Arrived on `/v1/logs`.
    Logs,
    /// Arrived on `/v1/traces`.
    Traces,
}

impl TelemetrySignal {
    /// The path an OTLP exporter appends to its endpoint for this signal.
    #[must_use]
    pub const fn otlp_path(self) -> &'static str {
        match self {
            Self::Metrics => "/v1/metrics",
            Self::Logs => "/v1/logs",
            Self::Traces => "/v1/traces",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_edition_changes_nothing() {
        let extension = NoExtension;
        assert_eq!(extension.edition(), "open");
        assert!(extension.sign_in_providers().is_empty());
        assert_eq!(extension.controller_notice(), None);
        extension.record_audit_event(&AuditEvent {
            organization_id: "org".to_owned(),
            actor_id: None,
            action: "token_sheet.submitted".to_owned(),
            target_id: None,
        });
        extension.forward_telemetry(&TelemetryBatch {
            organization_id: "org",
            signal: TelemetrySignal::Logs,
            content_type: "application/json",
            body: b"{}",
        });
    }

    #[test]
    fn each_signal_has_its_otlp_path() {
        assert_eq!(TelemetrySignal::Metrics.otlp_path(), "/v1/metrics");
        assert_eq!(TelemetrySignal::Logs.otlp_path(), "/v1/logs");
        assert_eq!(TelemetrySignal::Traces.otlp_path(), "/v1/traces");
    }
}
