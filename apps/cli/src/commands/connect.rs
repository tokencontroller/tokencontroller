//! `tc connect URL`: connects this computer to an organization with an
//! invite link, or to a self-hosted instance. The person confirms in the
//! browser, signed in as themselves; `tc` never sees a password.

use std::time::{Duration, Instant};

use crate::api::{Client, Poll, parse_connect_url};
use crate::app::App;
use crate::error::{Error, Result};
use crate::output;

/// How long the code stays valid on this side.
const WAIT: Duration = Duration::from_mins(15);

pub fn run(app: &mut App<'_>, url: &str) -> Result<()> {
    let (base, invite) = parse_connect_url(url)?;
    let client = Client::new(&base, None);
    let authorization = client.authorize(invite.as_deref(), &device_name())?;
    // Instructions go to standard error, so `--json` output stays one value.
    eprintln!(
        "Open {} and confirm the code {} to connect this computer.",
        authorization.verification_url, authorization.user_code
    );
    open_browser(&authorization.verification_url);

    let started = Instant::now();
    let interval = Duration::from_secs(authorization.interval_seconds.clamp(1, 30));
    let (token, organization, person) = loop {
        match client.poll(&authorization)? {
            Poll::Connected {
                token,
                organization,
                person,
            } => break (token, organization, person),
            Poll::Ended => {
                return Err(Error::usage(
                    "the code was declined or ran out; run tc connect again",
                ));
            }
            Poll::Pending => {
                if started.elapsed() > WAIT {
                    return Err(Error::usage(
                        "nobody confirmed the code in time; run tc connect again",
                    ));
                }
                std::thread::sleep(interval);
            }
        }
    };

    app.config.service = Some(base.clone());
    app.config.token = Some(token);
    app.home.write_config(&app.config)?;
    // New patterns and rules: read the session files again with them.
    app.refresh_context(true)?;
    let organization = app
        .context
        .organization
        .as_ref()
        .map(|o| o.name.clone())
        .or(organization);
    let person = app
        .context
        .person
        .as_ref()
        .map(|p| p.email.clone())
        .or(person);
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

/// A name for this computer, so the person can tell their devices apart
/// under Settings.
fn device_name() -> String {
    ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| "this computer".to_owned())
}

/// Opens the page in the default browser. If that fails the printed link is
/// still there. `TC_NO_BROWSER` turns it off, such as over SSH.
fn open_browser(url: &str) {
    if std::env::var_os("TC_NO_BROWSER").is_some() {
        return;
    }
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ignored = command
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
