//! The service's public API, as far as `tc` uses it (Reference: API). Any
//! service that speaks this API can be connected, the hosted one or a
//! self-hosted instance.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokencontroller_core::Rule;

use crate::context::Context;
use crate::error::{Error, Result};

/// A connection to a service.
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

/// What an invite link is for: the organization and the address invited.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct Invite {
    pub organization_name: String,
    pub email: String,
}

#[derive(serde::Deserialize)]
struct LinkSent {
    #[serde(default)]
    poll_token: Option<String>,
}

#[derive(serde::Deserialize)]
struct PollAnswer {
    #[serde(default)]
    session_token: Option<String>,
}

/// Who a token signs in as (`GET /me`).
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct Me {
    pub person: Named,
    pub organization: Named,
}

/// A person or an organization, as far as `tc` shows it.
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
pub struct Named {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// What the service saved from one token entry (`POST /token-entries`).
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
pub struct SavedEntry {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Option<u64>,
    #[serde(default)]
    pub cost: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub unknown_tags: Vec<String>,
}

/// One answer or a list of them, as the endpoint answers one entry or a
/// list.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum OneOrMany {
    Many(Vec<SavedEntry>),
    One(SavedEntry),
}

/// A submission as the service records it (`POST /submissions`).
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
pub struct SubmissionAnswer {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// The base address of a service from what a person pastes: an invite link
/// `https://app.tokencontroller.com/i/kestrel-4f2a`, an instance address
/// `https://tc.example.com`, or a bare host. Returns the base and the invite
/// code, if any.
pub fn parse_connect_url(url: &str) -> Result<(String, Option<String>)> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::usage(
            "give the invite link or the address of your instance",
        ));
    }
    let url = if url.contains("://") {
        url.to_owned()
    } else {
        format!("https://{url}")
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(Error::usage(format!("{url} is not an address")));
    };
    if scheme != "https" && scheme != "http" {
        return Err(Error::usage(format!(
            "{url} is not an http or https address"
        )));
    }
    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or(rest)
        .trim_end_matches('/');
    if let Some((base, code)) = rest.split_once("/i/") {
        let code = code.split('/').next().unwrap_or_default();
        if code.is_empty() {
            return Err(Error::usage(format!("{url} has no invite code after /i/")));
        }
        return Ok((format!("{scheme}://{base}"), Some(code.to_owned())));
    }
    if rest.ends_with("/i") {
        return Err(Error::usage(format!("{url} has no invite code after /i/")));
    }
    let base = rest;
    if base.is_empty() {
        return Err(Error::usage(format!("{url} has no host")));
    }
    Ok((format!("{scheme}://{base}"), None))
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent(concat!("tc/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            base: base.trim_end_matches('/').to_owned(),
            token,
            agent: config.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    fn send<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<(u16, Option<T>)> {
        let url = self.url(path);
        let network = |error: ureq::Error| Error::Network {
            url: url.clone(),
            reason: error.to_string(),
        };
        let authorization = self.token.as_ref().map(|token| format!("Bearer {token}"));
        let response = match (method, body) {
            ("GET", _) => {
                let mut request = self.agent.get(&url).header("Accept", "application/json");
                if let Some(value) = &authorization {
                    request = request.header("Authorization", value);
                }
                request.call().map_err(network)?
            }
            (_, body) => {
                let mut request = match method {
                    "PUT" => self.agent.put(&url),
                    _ => self.agent.post(&url),
                }
                .header("Accept", "application/json");
                if let Some(value) = &authorization {
                    request = request.header("Authorization", value);
                }
                request
                    .send_json(body.cloned().unwrap_or(serde_json::Value::Null))
                    .map_err(network)?
            }
        };
        let status = response.status().as_u16();
        let text = response.into_body().read_to_string().map_err(network)?;
        if (200..300).contains(&status) {
            let parsed = if text.trim().is_empty() {
                None
            } else {
                Some(serde_json::from_str(&text)?)
            };
            return Ok((status, parsed));
        }
        Err(Error::Service {
            status,
            message: error_message(status, &text),
        })
    }

    /// The organization and address an invite code is for.
    pub fn invite(&self, code: &str) -> Result<Invite> {
        let (_, invite) = self.send("GET", &format!("/invites/{code}"), None)?;
        invite.ok_or_else(|| Error::Data("the service sent no invite".into()))
    }

    /// Asks for a sign-in link for a local tool. Returns the token to poll
    /// with until the person has used the link.
    pub fn sign_in(&self, email: &str) -> Result<String> {
        let body = serde_json::json!({"email": email, "local_tool": true});
        let (_, sent) = self.send::<LinkSent>("POST", "/sign-in", Some(&body))?;
        sent.and_then(|sent| sent.poll_token)
            .ok_or_else(|| Error::Data("the service sent no poll token".into()))
    }

    /// The session token once the person has used the link, `None` while
    /// the service still waits.
    pub fn poll_sign_in(&self, poll_token: &str) -> Result<Option<String>> {
        let body = serde_json::json!({"poll_token": poll_token});
        let (_, answer) = self.send::<PollAnswer>("POST", "/sign-in/poll", Some(&body))?;
        Ok(answer.and_then(|answer| answer.session_token))
    }

    /// Who the token signs in as.
    pub fn me(&self) -> Result<Me> {
        let (_, me) = self.send("GET", "/me", None)?;
        me.ok_or_else(|| Error::Data("the service did not say who you are".into()))
    }

    /// The organization, person, tag values, rules, price table, cutoff and
    /// submissions.
    pub fn context(&self) -> Result<Context> {
        let (_, context) = self.send("GET", "/me/context", None)?;
        context.ok_or_else(|| Error::Data("the service sent no context".into()))
    }

    /// Replaces the person's ordered personal rules.
    pub fn put_rules(&self, rules: &[Rule]) -> Result<()> {
        let body = serde_json::json!({ "rules": rules });
        self.send::<serde_json::Value>("PUT", "/me/rules", Some(&body))?;
        Ok(())
    }

    /// Sends token entries, one per session, in one call.
    pub fn post_token_entries<T: Serialize>(&self, entries: &[T]) -> Result<Vec<SavedEntry>> {
        let body = serde_json::to_value(entries)?;
        let (_, answer) = self.send::<OneOrMany>("POST", "/token-entries", Some(&body))?;
        Ok(match answer {
            Some(OneOrMany::Many(saved)) => saved,
            Some(OneOrMany::One(saved)) => vec![saved],
            None => Vec::new(),
        })
    }

    /// Records a split on a session or a message.
    pub fn post_split<T: Serialize>(&self, split: &T) -> Result<()> {
        let body = serde_json::to_value(split)?;
        self.send::<serde_json::Value>("POST", "/splits", Some(&body))?;
        Ok(())
    }

    /// Submits every message of the person inside a window.
    pub fn post_submission(&self, from: &str, to: &str) -> Result<SubmissionAnswer> {
        let body = serde_json::json!({ "from": from, "to": to });
        let (_, answer) = self.send("POST", "/submissions", Some(&body))?;
        Ok(answer.unwrap_or_default())
    }
}

/// The service's error body is `{"error": "code", "message": "…"}`: the
/// message is written for people. Fall back to the code, then the status.
fn error_message(status: u16, body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|value| {
        ["message", "error", "detail"]
            .iter()
            .find_map(|field| match value.get(field) {
                Some(serde_json::Value::String(text)) => Some(text.clone()),
                Some(serde_json::Value::Object(inner)) => inner
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                _ => None,
            })
    });
    // Some refusals, such as a body the service cannot read, are plain text.
    let text = body.trim();
    let plain = (parsed.is_none() && !text.is_empty() && !text.starts_with('<'))
        .then(|| text.chars().take(300).collect::<String>());
    message.or(plain).unwrap_or_else(|| match status {
        401 => "not signed in; run tc connect again".to_owned(),
        403 => "this token may not do that".to_owned(),
        404 => "not found; is this a Token Controller service?".to_owned(),
        _ => "no details".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_links_and_instance_addresses_are_understood() {
        assert_eq!(
            parse_connect_url("https://app.tokencontroller.com/i/kestrel-4f2a").unwrap(),
            (
                "https://app.tokencontroller.com".to_owned(),
                Some("kestrel-4f2a".to_owned())
            )
        );
        assert_eq!(
            parse_connect_url("https://tc.example.com/").unwrap(),
            ("https://tc.example.com".to_owned(), None)
        );
        assert_eq!(
            parse_connect_url("tc.example.com").unwrap(),
            ("https://tc.example.com".to_owned(), None)
        );
        assert_eq!(
            parse_connect_url("http://localhost:8080/i/abc?x=1").unwrap(),
            ("http://localhost:8080".to_owned(), Some("abc".to_owned()))
        );
        assert!(parse_connect_url("").is_err());
        assert!(parse_connect_url("ftp://x").is_err());
        assert!(parse_connect_url("https://x/i/").is_err());
        assert!(parse_connect_url("https://").is_err());
    }

    #[test]
    fn service_errors_say_what_went_wrong() {
        assert_eq!(error_message(400, r#"{"error":"bad tag"}"#), "bad tag");
        assert_eq!(
            error_message(
                410,
                r#"{"error":"gone","message":"The sign-in link expired after 15 minutes. Sign in again."}"#
            ),
            "The sign-in link expired after 15 minutes. Sign in again."
        );
        assert_eq!(
            error_message(422, r#"{"error":{"message":"no such feature"}}"#),
            "no such feature"
        );
        assert_eq!(
            error_message(422, "Failed to deserialize the JSON body"),
            "Failed to deserialize the JSON body"
        );
        assert_eq!(
            error_message(401, "<html>nope</html>"),
            "not signed in; run tc connect again"
        );
        assert_eq!(error_message(403, ""), "this token may not do that");
        assert!(error_message(404, "").starts_with("not found"));
        assert_eq!(error_message(500, ""), "no details");
    }

    #[test]
    fn one_saved_entry_or_a_list_reads() {
        let one: OneOrMany =
            serde_json::from_str(r#"{"session_id":"s-1","messages":1,"cost":"0.03","currency":"USD","label":"direct","unknown_tags":["x"]}"#).unwrap();
        assert!(matches!(one, OneOrMany::One(ref saved) if saved.unknown_tags == ["x"]));
        let many: OneOrMany =
            serde_json::from_str(r#"[{"session_id":"a"},{"session_id":"b","cost":null}]"#).unwrap();
        assert!(matches!(many, OneOrMany::Many(ref saved) if saved.len() == 2));
    }
}
