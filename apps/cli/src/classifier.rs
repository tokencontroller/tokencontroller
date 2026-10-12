//! The optional classifier (Attribute: Classifier). When rules cannot place
//! a session, or placed it on a ticket that was only mentioned, `tc` runs the
//! program set with `tc config classifier`, writes a short digest of the
//! session as JSON to its standard input, and reads one line back: one of the
//! candidate tickets, or `none`. The digest never leaves the machine.
//!
//! The digest holds no prompt text, only what `tc` keeps anyway: the branch,
//! repository, title, summary, and how often and when each candidate ticket
//! was named.

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::entries::Entry;
use crate::error::{Error, Result};
use crate::keys::{KeyPattern, project_of};
use crate::output::utc;

/// How long a classifier may take. A local model loads in seconds.
const TIME_LIMIT: Duration = Duration::from_mins(1);

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Digest {
    pub agent_harness: String,
    pub started_at: String,
    pub ended_at: String,
    pub branch: Option<String>,
    pub repository: Option<String>,
    pub title: Option<String>,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub ticket: String,
    pub project: Option<String>,
    /// How often the ticket was named in prompts and the title.
    pub times_named: u64,
    /// Whether the branch or pull request title names it.
    pub in_branch_or_pull_request: bool,
}

/// The digest for an entry, or `None` when there is nothing to choose from.
pub fn digest(entry: &Entry, patterns: &[KeyPattern]) -> Option<Digest> {
    let signals = entry.main_signals();
    let keys = signals.all_keys(patterns);
    if keys.is_empty() {
        return None;
    }
    let branch_keys = signals
        .branch
        .as_deref()
        .map(|branch| crate::keys::find_all(patterns, branch))
        .unwrap_or_default();
    let candidates = keys
        .into_iter()
        .map(|ticket| Candidate {
            project: project_of(patterns, &ticket),
            times_named: signals
                .prompt_keys
                .iter()
                .find(|count| count.key == ticket)
                .map_or(0, |count| count.count),
            in_branch_or_pull_request: branch_keys.contains(&ticket)
                || signals.pull_request_title_keys.contains(&ticket),
            ticket,
        })
        .collect();
    Some(Digest {
        agent_harness: entry.row.harness.name().to_owned(),
        started_at: utc(entry.started_ms),
        ended_at: utc(entry.ended_ms),
        branch: signals.branch,
        repository: signals.repository,
        title: entry.summary.clone(),
        candidates,
    })
}

/// Runs the classifier and returns a candidate ticket or `none`.
pub fn ask(program: &str, digest: &Digest) -> Result<String> {
    let input = serde_json::to_string(digest)?;
    let mut child = command(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| Error::file(program, source))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A classifier that exits without reading is answered by its output.
        let _ignored = stdin.write_all(input.as_bytes());
    }
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|source| Error::file(program, source))?
            .is_some()
        {
            break;
        }
        if started.elapsed() > TIME_LIMIT {
            let _ignored = child.kill();
            return Err(Error::usage(format!(
                "the classifier {program} took longer than {} seconds",
                TIME_LIMIT.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut answer = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout
            .read_to_string(&mut answer)
            .map_err(|source| Error::file(program, source))?;
    }
    let answer = answer.lines().next().unwrap_or_default().trim().to_owned();
    if answer == "none" || digest.candidates.iter().any(|c| c.ticket == answer) {
        Ok(answer)
    } else {
        Err(Error::Data(format!(
            "the classifier answered {answer:?}, which is not one of the candidates or none"
        )))
    }
}

/// The classifier runs through the shell, so a setting like
/// `~/bin/pick-ticket --model small` works as typed.
fn command(program: &str) -> Command {
    let expanded = match program.split_once(' ') {
        Some((first, rest)) => format!(
            "{} {rest}",
            crate::home::expand_tilde(first).to_string_lossy()
        ),
        None => crate::home::expand_tilde(program)
            .to_string_lossy()
            .into_owned(),
    };
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", &expanded]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", &expanded]);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> Digest {
        Digest {
            agent_harness: "claude-code".into(),
            started_at: String::new(),
            ended_at: String::new(),
            branch: None,
            repository: None,
            title: None,
            candidates: vec![
                Candidate {
                    ticket: "KD-1".into(),
                    project: None,
                    times_named: 1,
                    in_branch_or_pull_request: false,
                },
                Candidate {
                    ticket: "KD-2".into(),
                    project: None,
                    times_named: 3,
                    in_branch_or_pull_request: false,
                },
            ],
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_answer_must_be_a_candidate_or_none() {
        assert_eq!(ask("echo KD-2", &digest()).unwrap(), "KD-2");
        assert_eq!(ask("printf 'none\\nextra'", &digest()).unwrap(), "none");
        assert!(ask("echo KD-9", &digest()).is_err());
        assert!(ask("cat > /dev/null; echo KD-1", &digest()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn the_classifier_reads_the_digest_on_standard_input() {
        let program = "grep -q '\"times_named\":3' && echo KD-2 || echo none";
        assert_eq!(ask(program, &digest()).unwrap(), "KD-2");
    }
}
