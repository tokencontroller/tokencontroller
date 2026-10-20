//! The `tokencontroller` program: runs the service (`serve`, the default),
//! loads the example organization (`seed`), prints the OpenAPI document
//! (`openapi`), prints a sign-in link for a member (`sign-in-link EMAIL`), or
//! shows what the telemetry receiver makes of a saved export request
//! (`otlp-decode SIGNAL FILE`), for the agent harness checks.

use std::io::{Read as _, Write as _};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context as _;
use tokencontroller_core::NoExtension;
use tokencontroller_server::ingest::otlp::Signal;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let result = match command.as_deref() {
        None | Some("serve") => tokencontroller_server::serve(Arc::new(NoExtension)).await,
        Some("seed") => tokencontroller_server::seed().await,
        Some("openapi") => write_openapi(),
        Some("sign-in-link") => match args.next() {
            Some(email) => write_sign_in_link(&email).await,
            None => Err(anyhow::anyhow!("usage: tokencontroller sign-in-link EMAIL")),
        },
        Some("otlp-decode") => decode_otlp(args.collect()),
        Some(other) => Err(anyhow::anyhow!(
            "unknown command {other:?}: use `serve` (the default), `seed`, `openapi`, `sign-in-link EMAIL` or `otlp-decode SIGNAL FILE`"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(std::io::stderr(), "error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn write_sign_in_link(email: &str) -> anyhow::Result<()> {
    let link = tokencontroller_server::sign_in_link(email).await?;
    writeln!(std::io::stdout().lock(), "{link}")?;
    Ok(())
}

fn write_openapi() -> anyhow::Result<()> {
    let document = tokencontroller_server::openapi_json()?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(document.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}

/// `otlp-decode logs|traces|metrics [--content-type TYPE] [--content-encoding
/// ENCODING] FILE`: reads the saved body of one export request as the
/// receiver would and prints the messages it holds as JSON. `-` reads
/// standard input. Without a content type the body is protobuf.
fn decode_otlp(args: Vec<String>) -> anyhow::Result<()> {
    const USAGE: &str = "usage: tokencontroller otlp-decode logs|traces|metrics [--content-type TYPE] [--content-encoding ENCODING] FILE";
    let mut words = args.into_iter();
    let signal = match words.next().as_deref() {
        Some("logs") => Signal::Logs,
        Some("traces") => Signal::Traces,
        Some("metrics") => Signal::Metrics,
        _ => anyhow::bail!("{USAGE}"),
    };
    let mut content_type = None;
    let mut content_encoding = None;
    let mut file = None;
    while let Some(word) = words.next() {
        match word.as_str() {
            "--content-type" => content_type = words.next(),
            "--content-encoding" => content_encoding = words.next(),
            _ => file = Some(word),
        }
    }
    let Some(file) = file else {
        anyhow::bail!("{USAGE}");
    };
    let body = if file == "-" {
        let mut body = Vec::new();
        std::io::stdin().read_to_end(&mut body)?;
        body
    } else {
        std::fs::read(&file).with_context(|| format!("cannot read {file}"))?
    };
    let described = tokencontroller_server::ingest::otlp::describe(
        signal,
        content_type.as_deref(),
        content_encoding.as_deref(),
        &body,
    )?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &described)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
