//! `tc connect URL`: connects this computer to an organization with an
//! invite link, or to a self-hosted instance. It signs in with a magic link,
//! as the web app does: the service mails a link, the person presses Sign
//! in, and `tc` collects a session token for this machine. `tc` never sees a
//! password.

use std::io::BufRead;
use std::time::{Duration, Instant};

use crate::api::{Client, parse_connect_url};
use crate::app::App;
use crate::error::{Error, Result};
use crate::output;

/// How long a sign-in link works.
const WAIT: Duration = Duration::from_mins(15);

/// How often to ask whether the link was used.
const POLL_EVERY: Duration = Duration::from_secs(2);

pub fn run(app: &mut App<'_>, url: &str, input: &mut dyn BufRead) -> Result<()> {
    let (base, invite) = parse_connect_url(url)?;
    let client = Client::new(&base, None);
    // Instructions go to standard error, so `--json` output stays one value.
    let email = match invite {
        Some(code) => {
            let invite = client.invite(&code)?;
            eprintln!("Joining {} as {}.", invite.organization_name, invite.email);
            invite.email
        }
        None => ask_email(&base, input)?,
    };
    let poll_token = client.sign_in(&email)?;
    eprintln!("Sent a sign-in link to {email}. Open the mail and press Sign in; tc waits here.");

    let started = Instant::now();
    let token = loop {
        if let Some(token) = client.poll_sign_in(&poll_token)? {
            break token;
        }
        if started.elapsed() > WAIT {
            return Err(Error::usage(
                "the sign-in link was not used in time; run tc connect again",
            ));
        }
        std::thread::sleep(POLL_EVERY);
    };

    app.config.service = Some(base.clone());
    app.config.token = Some(token.clone());
    app.home.write_config(&app.config)?;
    let me = Client::new(&base, Some(token)).me()?;
    let organization = me.organization.name;
    let person = me.person.email.or(Some(email));
    // New patterns and rules: placing uses them from the next command on.
    if let Err(error) = app.refresh_context(true) {
        eprintln!("tc: connected, but your organization's rules could not be read yet: {error}");
    }
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({
                "connected": true,
                "service": base,
                "organization": organization,
                "person": person,
            }),
        );
    }
    writeln!(
        app.out,
        "connected to {} as {}",
        organization.as_deref().unwrap_or(&base),
        person.as_deref().unwrap_or("you")
    )?;
    Ok(())
}

/// The address to sign in with, typed by the person.
fn ask_email(base: &str, input: &mut dyn BufRead) -> Result<String> {
    eprint!("Your email address at {base}: ");
    let mut line = String::new();
    input
        .read_line(&mut line)
        .map_err(|source| Error::file("standard input", source))?;
    let email = line.trim();
    if email.contains('@') {
        Ok(email.to_owned())
    } else {
        Err(Error::usage(
            "tc connect needs the email address you sign in with, or your invite link",
        ))
    }
}
