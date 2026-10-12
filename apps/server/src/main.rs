//! The `tokencontroller` program: runs the service (`serve`, the default),
//! loads the example organization (`seed`), prints the OpenAPI document
//! (`openapi`), or prints a sign-in link for a member (`sign-in-link EMAIL`).

use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use tokencontroller_core::NoExtension;

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
        Some(other) => Err(anyhow::anyhow!(
            "unknown command {other:?}: use `serve` (the default), `seed`, `openapi` or `sign-in-link EMAIL`"
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
