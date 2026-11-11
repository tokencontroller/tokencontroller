//! Activities: every record from a session that is not a model call, such
//! as a tool run or a counter of commits (ADR 0028).
//!
//! A record with token counts is a message; every other record is an
//! activity with a kind name, its session, its time and its signals. A new
//! kind needs no change to the model. No formula reads activities yet. The
//! word is not "event", since an event is an entry in the organization's
//! log.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::message::{SessionId, Signals};
use crate::tags::TagValue;

/// One activity, as a usage endpoint read it from a received record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// The kind, as the source names it, such as
    /// `claude_code.tool_result` or `claude_code.commit.count`.
    pub kind: String,
    /// The session the activity belongs to, when the source names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// The person whose session it is.
    pub person: TagValue,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// The product the agent ran on, such as `claude-code`.
    pub agent_harness: String,
    /// Everything else that came with it, by name. Never content text.
    #[serde(default)]
    pub signals: Signals,
    /// The received record the activity was read from, such as `rcv_…`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn an_activity_reads_and_writes_as_json() {
        let json = r#"{
            "kind": "claude_code.tool_result", "session_id": "s1", "person": "mara",
            "at": "2026-10-05T09:12:00Z", "agent_harness": "claude-code",
            "signals": {"tool_name": "Bash", "success": "true"},
            "received": "rcv_1"
        }"#;
        let read: Activity = serde_json::from_str(json).unwrap();
        assert_eq!(read.kind, "claude_code.tool_result");
        assert_eq!(read.session_id.as_ref().unwrap().as_str(), "s1");
        assert_eq!(read.at, datetime!(2026-10-05 09:12 UTC));
        assert_eq!(
            read.signals.other.get("tool_name"),
            Some(&serde_json::json!("Bash"))
        );
        let again: Activity = serde_json::from_str(&serde_json::to_string(&read).unwrap()).unwrap();
        assert_eq!(again, read);
        // Session and received record are optional.
        let bare: Activity = serde_json::from_str(
            r#"{"kind": "commit.count", "person": "mara",
                "at": "2026-10-05T09:12:00Z", "agent_harness": "claude-code"}"#,
        )
        .unwrap();
        assert_eq!(bare.session_id, None);
        assert_eq!(bare.received, None);
        let written = serde_json::to_string(&bare).unwrap();
        assert!(!written.contains("session_id"));
        assert!(!written.contains("received"));
    }
}
