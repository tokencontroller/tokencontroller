//! Mail the service sends over SMTP (`TC_SMTP_URL`, ADR 0002): sign-in links,
//! invites, reminders. Every mail has a plain-text and an HTML part.
//!
//! The service records each attempt in `mail_deliveries` (kind and outcome
//! only, never the address or the content), so the status endpoint can tell
//! when sign-in mail stops arriving.

use std::time::Duration;

use anyhow::Context as _;
use lettre::message::{Mailbox, MultiPart};
use lettre::{AsyncSmtpTransport, AsyncTransport as _, Message, Tokio1Executor};
use sqlx::PgPool;

use crate::Config;

/// How long one SMTP conversation may take.
const SMTP_TIMEOUT: Duration = Duration::from_secs(20);

/// One mail to one person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    /// A short name for what it is, such as `sign_in`, kept with the outcome.
    pub kind: &'static str,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

/// What became of a mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The mail server accepted it.
    Sent,
    /// `TC_SMTP_URL` is not set, so nothing was sent.
    NoMailServer,
}

/// Sends `mail` and records the outcome.
///
/// # Errors
///
/// Fails if the sender or the address is not a valid mailbox or the mail
/// server refuses the mail. The failure is recorded before it is returned.
pub async fn send(db: &PgPool, config: &Config, mail: Mail) -> anyhow::Result<Delivery> {
    let Some(smtp_url) = config.smtp_url.as_deref() else {
        tracing::warn!(
            kind = mail.kind,
            "mail not sent: TC_SMTP_URL is not set. Run `tokencontroller sign-in-link EMAIL` for a sign-in link"
        );
        return Ok(Delivery::NoMailServer);
    };
    let kind = mail.kind;
    let result = deliver(smtp_url, &sender(config), mail).await;
    record(db, kind, result.is_ok()).await;
    match result {
        Ok(()) => Ok(Delivery::Sent),
        Err(error) => {
            tracing::error!(kind, "could not send mail: {error:#}");
            Err(error)
        }
    }
}

/// The `From` of every mail: `TC_SMTP_FROM`, or `noreply@` the host of
/// `TC_PUBLIC_URL`.
#[must_use]
pub fn sender(config: &Config) -> String {
    config
        .smtp_from
        .clone()
        .unwrap_or_else(|| format!("Token Controller <noreply@{}>", host_of(&config.public_url)))
}

/// The brand mark for mail headers: `logo-light-48.png`, which the web app
/// serves next to the service (`apps/web/public`). Mail apps show no SVG.
#[must_use]
pub fn logo_url(public_url: &str) -> String {
    format!("{}/logo-light-48.png", public_url.trim_end_matches('/'))
}

/// The host part of a URL such as `https://tc.example.com:8443/x`.
#[must_use]
pub fn host_of(url: &str) -> &str {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    authority.split(':').next().unwrap_or(authority)
}

async fn deliver(smtp_url: &str, from: &str, mail: Mail) -> anyhow::Result<()> {
    let from: Mailbox = from
        .parse()
        .with_context(|| format!("TC_SMTP_FROM is not a valid sender: {from:?}"))?;
    let to: Mailbox = mail
        .to
        .parse()
        .with_context(|| format!("not a valid address: {:?}", mail.to))?;
    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(mail.subject)
        .multipart(MultiPart::alternative_plain_html(mail.text, mail.html))
        .context("could not build the mail")?;
    let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(smtp_url)
        .context("TC_SMTP_URL is not a valid SMTP URL")?
        .timeout(Some(SMTP_TIMEOUT))
        .build();
    transport
        .send(message)
        .await
        .context("the mail server refused the mail")?;
    Ok(())
}

async fn record(db: &PgPool, kind: &str, sent: bool) {
    if let Err(error) = sqlx::query!(
        "INSERT INTO mail_deliveries (kind, sent) VALUES ($1, $2)",
        kind,
        sent
    )
    .execute(db)
    .await
    {
        tracing::warn!(%error, "could not record a mail delivery");
    }
}

/// Fills `{{name}}` placeholders. HTML templates get their values escaped.
#[must_use]
pub fn render(template: &str, values: &[(&str, &str)], html: bool) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            let value = if html {
                escape_html(value)
            } else {
                (*value).to_owned()
            };
            text.replace(&format!("{{{{{name}}}}}"), &value)
        })
}

/// Escapes text for use inside HTML elements and quoted attributes.
#[must_use]
pub fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(smtp_from: Option<&str>) -> Config {
        Config {
            database_url: "postgres://unused".to_owned(),
            public_url: "https://tc.example.com:8443".to_owned(),
            secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
            smtp_url: None,
            smtp_from: smtp_from.map(str::to_owned),
            price_table: None,
            rates_refresh: true,
            classifier: None,
        }
    }

    #[test]
    fn the_sender_defaults_to_the_public_host() {
        assert_eq!(
            sender(&config(None)),
            "Token Controller <noreply@tc.example.com>"
        );
        assert_eq!(
            sender(&config(Some("Token Controller <mail@tokencontroller.com>"))),
            "Token Controller <mail@tokencontroller.com>"
        );
    }

    #[test]
    fn hosts_come_without_scheme_port_or_path() {
        assert_eq!(
            host_of("https://app.tokencontroller.com"),
            "app.tokencontroller.com"
        );
        assert_eq!(host_of("http://localhost:8080/x"), "localhost");
        assert_eq!(host_of("tc.example.com"), "tc.example.com");
    }

    #[test]
    fn templates_escape_html_but_not_text() {
        let values = [("name", "<Kestrel & Co>")];
        assert_eq!(
            render("Hi {{name}}", &values, true),
            "Hi &lt;Kestrel &amp; Co&gt;"
        );
        assert_eq!(render("Hi {{name}}", &values, false), "Hi <Kestrel & Co>");
        assert_eq!(render("{{missing}}", &values, false), "{{missing}}");
    }

    #[test]
    fn quotes_are_escaped_for_attributes() {
        assert_eq!(escape_html(r#""it's""#), "&quot;it&#39;s&quot;");
    }
}
