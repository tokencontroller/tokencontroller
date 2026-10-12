//! The service's public API, as far as `tc` uses it (Reference: API). Any
//! service that speaks this API can be connected, the hosted one or a
//! self-hosted instance.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::context::{Context, Rule};
use crate::error::{Error, Result};

/// A connection to a service.
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

/// Where `tc connect` sends the person to confirm this computer.
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct Authorization {
    pub id: String,
    pub user_code: String,
    pub verification_url: String,
    pub poll_secret: String,
    #[serde(default = "default_interval")]
    pub interval_seconds: u64,
}

fn default_interval() -> u64 {
    2
}

/// The answer to one poll while the person confirms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Pending,
    Connected {
        token: String,
        organization: Option<String>,
        person: Option<String>,
    },
    Ended,
}

#[derive(serde::Deserialize)]
struct Granted {
    token: String,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    person: Option<String>,
}

/// What the service says about a submitted token sheet.
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
pub struct SubmitAnswer {
    #[serde(default)]
    pub token_entries: Vec<SubmittedEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
pub struct SubmittedEntry {
    pub local_id: String,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
pub struct CreatedEntry {
    #[serde(default)]
    pub id: Option<String>,
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
        if status == 202 || status == 410 {
            return Ok((status, None));
        }
        Err(Error::Service {
            status,
            message: error_message(status, &text),
        })
    }

    /// Starts connecting this computer: the service gives a code the person
    /// confirms in the browser.
    pub fn authorize(&self, invite: Option<&str>, device: &str) -> Result<Authorization> {
        let body = serde_json::json!({"invite": invite, "device": device});
        let (_, answer) = self.send("POST", "/cli/authorizations", Some(&body))?;
        answer.ok_or_else(|| Error::Data("the service sent no code to confirm".into()))
    }

    /// Asks whether the person confirmed yet.
    pub fn poll(&self, authorization: &Authorization) -> Result<Poll> {
        let body = serde_json::json!({"poll_secret": authorization.poll_secret});
        let path = format!("/cli/authorizations/{}/token", authorization.id);
        let (status, answer) = self.send::<Granted>("POST", &path, Some(&body))?;
        Ok(match (status, answer) {
            (202, _) => Poll::Pending,
            (410, _) => Poll::Ended,
            (_, Some(granted)) => Poll::Connected {
                token: granted.token,
                organization: granted.organization,
                person: granted.person,
            },
            (_, None) => Poll::Pending,
        })
    }

    /// The organization, person, projects, rules and cadence.
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

    /// Submits the token sheet.
    pub fn submit<T: Serialize>(&self, entries: &[T]) -> Result<SubmitAnswer> {
        let body = serde_json::json!({ "token_entries": entries });
        let (_, answer) = self.send("POST", "/token-sheets/submit", Some(&body))?;
        Ok(answer.unwrap_or_default())
    }

    /// Reports one automatic token entry, with a service token.
    pub fn post_token_entry<T: Serialize>(&self, entry: &T) -> Result<CreatedEntry> {
        let body = serde_json::to_value(entry)?;
        let (_, answer) = self.send("POST", "/token-entries", Some(&body))?;
        Ok(answer.unwrap_or_default())
    }
}

/// The service's error body is `{"error": "…"}` or `{"message": "…"}`; fall
/// back to the status.
fn error_message(status: u16, body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|value| {
        ["error", "message", "detail"]
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
    message.unwrap_or_else(|| match status {
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
            error_message(422, r#"{"error":{"message":"no such ticket"}}"#),
            "no such ticket"
        );
        assert_eq!(
            error_message(401, "nope"),
            "not signed in; run tc connect again"
        );
        assert_eq!(error_message(403, ""), "this token may not do that");
        assert!(error_message(404, "").starts_with("not found"));
        assert_eq!(error_message(500, ""), "no details");
    }
}
