//! Sending mail: support answers and notices over SMTP (`TC_SMTP_URL`), the
//! newsletter through the Resend API. Newsletter mail never goes over SMTP,
//! so a spam complaint about it cannot block anyone's sign-in link.

use lettre::message::header::{HeaderName, HeaderValue};
use lettre::message::{Mailbox, Message};
use lettre::{AsyncSmtpTransport, AsyncTransport as _, Tokio1Executor};
use serde_json::json;

use super::settings::Settings;

/// One mail to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub from: String,
    pub to: Mailbox,
    pub reply_to: Option<String>,
    pub subject: String,
    pub text: String,
    /// The Message-ID without angle brackets.
    pub message_id: Option<String>,
    /// Message-IDs this mail answers, oldest first, without angle brackets.
    pub references: Vec<String>,
    /// Extra headers such as `List-Unsubscribe`.
    pub headers: Vec<(&'static str, String)>,
}

/// Builds the mail as it goes over SMTP.
///
/// # Errors
///
/// When an address or header is invalid.
pub fn build(mail: &Outgoing) -> anyhow::Result<Message> {
    let mut builder = Message::builder()
        .from(mail.from.parse::<Mailbox>()?)
        .to(mail.to.clone())
        .subject(mail.subject.clone());
    if let Some(reply_to) = &mail.reply_to {
        builder = builder.reply_to(reply_to.parse::<Mailbox>()?);
    }
    if let Some(id) = &mail.message_id {
        builder = builder.message_id(Some(format!("<{id}>")));
    }
    if let Some(last) = mail.references.last() {
        builder = builder.in_reply_to(format!("<{last}>"));
        let references: Vec<String> = mail.references.iter().map(|id| format!("<{id}>")).collect();
        builder = builder.references(references.join(" "));
    }
    for (name, value) in &mail.headers {
        builder = builder.raw_header(HeaderValue::new(
            HeaderName::new_from_ascii_str(name),
            value.clone(),
        ));
    }
    Ok(builder.body(mail.text.clone())?)
}

/// Sends one mail over SMTP.
///
/// # Errors
///
/// When the mail cannot be built, the SMTP URL is wrong, or the server refuses.
pub async fn send_smtp(smtp_url: &str, mail: &Outgoing) -> anyhow::Result<()> {
    let message = build(mail)?;
    let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(smtp_url)?.build();
    transport.send(message).await?;
    Ok(())
}

/// Whether Resend took the mail, or asked us to come back later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    Sent,
    /// Rate or daily limit reached; try again in a while.
    Later,
}

/// Sends one mail through the Resend API.
///
/// # Errors
///
/// When the newsletter is not configured, Resend cannot be reached, or it
/// refuses the mail for any reason other than a limit.
pub async fn send_resend(settings: &Settings, mail: &Outgoing) -> anyhow::Result<Sent> {
    let key = settings
        .resend_api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("TC_RESEND_API_KEY is not set"))?;
    let headers: serde_json::Map<String, serde_json::Value> = mail
        .headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), json!(value)))
        .collect();
    let mut body = json!({
        "from": mail.from,
        "to": [mail.to.to_string()],
        "subject": mail.subject,
        "text": mail.text,
        "headers": headers,
    });
    if let (Some(reply_to), Some(object)) = (&mail.reply_to, body.as_object_mut()) {
        object.insert("reply_to".to_owned(), json!(reply_to));
    }
    let response = super::claude::http()?
        .post(format!("{}/emails", settings.resend_api_url))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    if status.is_success() {
        return Ok(Sent::Sent);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Ok(Sent::Later);
    }
    let text: String = response.text().await?.chars().take(300).collect();
    anyhow::bail!("Resend refused the mail with status {status}: {text}")
}

/// The domain of a sender such as `Support <support@example.com>`, for
/// Message-IDs.
#[must_use]
pub fn domain_of(sender: &str) -> String {
    sender.parse::<Mailbox>().map_or_else(
        |_| "localhost".to_owned(),
        |mailbox| mailbox.email.domain().to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> Outgoing {
        Outgoing {
            from: "Support <support@example.com>".to_owned(),
            to: "Ana Ortner <ana@kestrel.example>".parse().unwrap(),
            reply_to: Some("support@example.com".to_owned()),
            subject: "Re: Bills".to_owned(),
            text: "Hello".to_owned(),
            message_id: Some("abc@example.com".to_owned()),
            references: vec![
                "first@kestrel.example".to_owned(),
                "second@example.com".to_owned(),
            ],
            headers: vec![(
                "List-Unsubscribe-Post",
                "List-Unsubscribe=One-Click".to_owned(),
            )],
        }
    }

    #[test]
    fn answers_thread_under_the_mail_they_answer() {
        let formatted = String::from_utf8(build(&mail()).unwrap().formatted()).unwrap();
        assert!(formatted.contains("Message-ID: <abc@example.com>"));
        assert!(formatted.contains("In-Reply-To: <second@example.com>"));
        assert!(formatted.contains("References: <first@kestrel.example> <second@example.com>"));
        assert!(formatted.contains("Reply-To: support@example.com"));
        assert!(formatted.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
    }

    #[test]
    fn a_new_mail_has_no_reply_headers() {
        let mut new = mail();
        new.references.clear();
        let formatted = String::from_utf8(build(&new).unwrap().formatted()).unwrap();
        assert!(!formatted.contains("In-Reply-To"));
        assert!(!formatted.contains("References"));
    }

    #[test]
    fn message_ids_use_the_sender_domain() {
        assert_eq!(domain_of("Support <support@example.com>"), "example.com");
        assert_eq!(domain_of("nonsense"), "localhost");
    }
}
