//! Who charged for a token entry. Reconciliation checks each bill on its own,
//! so every token entry names its provider, spelled as bills spell it.

/// Every provider a token entry or a bill may name.
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

/// The provider of a model call, from what the door knows: the provider it
/// named, the agent harness and the model's name. Claude through Bedrock is
/// `aws_bedrock`, not `anthropic`.
#[must_use]
pub fn infer(named: Option<&str>, agent_harness: &str, model: &str) -> &'static str {
    if let Some(named) = named
        && let Some(known) = PROVIDERS.iter().find(|known| **known == named)
    {
        return known;
    }
    match agent_harness {
        "copilot-cli" => return "github",
        "cursor" => return "cursor",
        _ => {}
    }
    let model = model.to_ascii_lowercase();
    // Cloud model IDs carry the cloud in their form: Bedrock IDs such as
    // `anthropic.claude-…` or `us.anthropic.claude-…`, Vertex IDs with `@`.
    if model.contains("anthropic.") || model.starts_with("bedrock/") {
        return "aws_bedrock";
    }
    if model.starts_with("vertex_ai/") || (model.starts_with("claude") && model.contains('@')) {
        return "gcp_vertex";
    }
    if model.starts_with("azure/") {
        return "azure_foundry";
    }
    let name = model.rsplit('/').next().unwrap_or(&model);
    if name.starts_with("claude") {
        "anthropic"
    } else if name.starts_with("gpt")
        || name.starts_with("codex")
        || name.starts_with("o1")
        || name.starts_with("o3")
        || name.starts_with("o4")
        || name.starts_with("chatgpt")
    {
        "openai"
    } else if name.starts_with("gemini") || name.starts_with("gemma") {
        "google"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_provider_wins_when_known() {
        assert_eq!(
            infer(Some("aws_bedrock"), "claude-code", "claude-sonnet-5"),
            "aws_bedrock"
        );
        assert_eq!(
            infer(Some("acme"), "claude-code", "claude-sonnet-5"),
            "anthropic"
        );
    }

    #[test]
    fn the_model_name_decides_otherwise() {
        assert_eq!(infer(None, "claude-code", "claude-sonnet-5"), "anthropic");
        assert_eq!(
            infer(None, "claude-code", "us.anthropic.claude-sonnet-4-5-v1:0"),
            "aws_bedrock"
        );
        assert_eq!(
            infer(None, "claude-code", "claude-sonnet-4-5@20250929"),
            "gcp_vertex"
        );
        assert_eq!(infer(None, "codex-cli", "gpt-5-codex"), "openai");
        assert_eq!(infer(None, "support-desk", "openai/o3-mini"), "openai");
        assert_eq!(infer(None, "gemini-cli", "gemini-2.5-pro"), "google");
        assert_eq!(infer(None, "support-desk", "azure/gpt-5"), "azure_foundry");
        assert_eq!(infer(None, "support-desk", "llama-4"), "other");
    }

    #[test]
    fn copilot_and_cursor_charge_for_their_own_calls() {
        assert_eq!(infer(None, "copilot-cli", "claude-sonnet-5"), "github");
        assert_eq!(infer(None, "cursor", "gpt-5"), "cursor");
    }
}
