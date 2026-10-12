//! `tc`, the Token Controller command line (ADR 0010, ADR 0015).
//!
//! It reads the session files of Claude Code and Codex on this machine, keeps
//! draft token entries in a local `SQLite` file, places them with the rules,
//! asks the optional classifier, and submits token sheets over the public
//! API. It works with no service connected. There is no background process:
//! each command reads what is new and then does its job.

mod amounts;
mod api;
mod app;
mod classifier;
mod cli;
mod commands;
mod context;
mod entries;
mod error;
mod home;
mod keys;
mod output;
mod placing;
mod prices;
mod repo;
mod sources;
mod store;
mod sync;
mod tokens;

use std::process::ExitCode;

fn main() -> ExitCode {
    let matches = match cli::command().try_get_matches() {
        Ok(matches) => matches,
        // Help and the version go to standard output and succeed; a wrong
        // command line is the caller's mistake, exit code 1 like the rest.
        Err(error) => {
            let code = u8::from(error.use_stderr());
            // Nothing is left to report if the terminal is gone.
            let _ignored = error.print();
            return ExitCode::from(code);
        }
    };
    let args = match cli::from_matches(&matches) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("tc: {error}");
            return ExitCode::from(error.exit_code());
        }
    };
    let mut stdout = std::io::stdout().lock();
    match commands::run(args, &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error::Error::Output(error)) if error.kind() == std::io::ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("tc: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
