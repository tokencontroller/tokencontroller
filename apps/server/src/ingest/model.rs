//! The shapes every door produces before anything is stored: one model call
//! as a [`Message`] (ADR 0006), with its [`TokenCounts`].

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::Money;
use utoipa::ToSchema;

/// Token counts by kind, as the token entry schema names them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenCounts {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_write: u64,
    #[serde(default)]
    pub cache_read: u64,
}

impl TokenCounts {
    /// All four kinds added up, or `None` if the sum does not fit.
    #[must_use]
    pub fn total(self) -> Option<u64> {
        self.input
            .checked_add(self.output)?
            .checked_add(self.cache_write)?
            .checked_add(self.cache_read)
    }

    /// Both counts added kind by kind, or `None` if a sum does not fit.
    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_add(other.input)?,
            output: self.output.checked_add(other.output)?,
            cache_write: self.cache_write.checked_add(other.cache_write)?,
            cache_read: self.cache_read.checked_add(other.cache_read)?,
        })
    }
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
