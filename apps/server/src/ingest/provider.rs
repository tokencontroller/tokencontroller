//! Providers: who charges for a model call, spelled as bills spell it. The
//! provider is never guessed from the model's name (ADR 0028): a message
//! carries the one the agent or the sender reported, and its contract's
//! provider wins.

/// The providers the price table names rows under. A message or a bill may
/// name any other provider in the same spelling.
pub const PROVIDERS: [&str; 9] = [
    "anthropic",
    "openai",
    "google",
    "aws_bedrock",
    "gcp_vertex",
    "azure_foundry",
    "github",
    "cursor",
    "other",
];
