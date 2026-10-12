//! The shapes every door produces before anything is stored: one model call
//! as a [`TelemetryMessage`] (ADR 0006), with its [`TokenCounts`].

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::Money;
use utoipa::ToSchema;

pub use tokencontroller_core::TokenCounts;

/// Token counts by kind, as the OpenAPI document shows [`TokenCounts`].
#[derive(Debug, Clone, Copy, Default, ToSchema)]
#[schema(as = TokenCounts)]
pub struct TokenCountsSchema {
    #[schema(default = 0)]
    pub input: u64,
    #[schema(default = 0)]
    pub output: u64,
    #[schema(default = 0)]
    pub cache_write: u64,
    #[schema(default = 0)]
    pub cache_read: u64,
}

/// The agent harnesses telemetry arrives from. Token entries from the
/// endpoint or a CSV file may name any other agent harness as free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TelemetryHarness {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    CopilotCli,
    Cursor,
}

impl TelemetryHarness {
    /// The name stored as the token entry's agent harness.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex-cli",
            Self::GeminiCli => "gemini-cli",
            Self::CopilotCli => "copilot-cli",
            Self::Cursor => "cursor",
        }
    }
}

/// One model call seen through telemetry, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryMessage {
    pub harness: TelemetryHarness,
    /// The agent harness's session ID; joins telemetry to token entries.
    pub session_id: String,
    /// The ID of this model call within the session, unique per session.
    pub message_id: String,
    pub model: String,
    pub occurred_at: OffsetDateTime,
    pub tokens: TokenCounts,
    /// The cost the agent harness reported for this call, if any.
    pub reported_cost: Option<Money>,
    /// Who made the call: the email address the harness reports, if any.
    pub person_email: Option<String>,
    /// The harness's own account or user ID, if any.
    pub person_account_id: Option<String>,
    /// The repository, such as `github.com/acme/storefront`, if reported.
    pub repository: Option<String>,
    /// Resource and event attributes, by name, that may become tags. Only
    /// names on the organization's tag list are kept later.
    pub attributes: std::collections::BTreeMap<String, String>,
}
