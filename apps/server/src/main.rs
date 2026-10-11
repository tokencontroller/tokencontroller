//! The `tokencontroller` program: runs the service, or prints its OpenAPI
//! document with `tokencontroller openapi`.

use std::io::Write as _;
use std::process::ExitCode;
use std::sync::Arc;

use tokencontroller_core::NoExtension;

#[tokio::main]
async fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        None | Some("serve") => tokencontroller_server::serve(Arc::new(NoExtension)).await,
        Some("openapi") => write_openapi(),
        Some(other) => Err(anyhow::anyhow!(
            "unknown command {other:?}: use `serve` (the default) or `openapi`"
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

fn write_openapi() -> anyhow::Result<()> {
    let document = tokencontroller_server::openapi_json()?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(document.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}
