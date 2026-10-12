//! `tc` end to end: the real binary, real session files in a throwaway home
//! folder, and a small fake service where a test needs one.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// A machine with a home folder, two repositories and session files of
/// Claude Code and Codex in them.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> TestResult<Self> {
        let machine = Self {
            dir: tempfile::tempdir()?,
        };
        machine.repository("storefront", "git@github.com:kestrel/storefront.git")?;
        machine.repository("ops", "https://github.com/kestrel/ops.git")?;
        Ok(machine)
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn work(&self, name: &str) -> PathBuf {
        self.dir.path().join("work").join(name)
    }

    fn repository(&self, name: &str, origin: &str) -> TestResult<()> {
        let git = self.work(name).join(".git");
        std::fs::create_dir_all(&git)?;
        std::fs::write(
            git.join("config"),
            format!("[remote \"origin\"]\n\turl = {origin}\n"),
        )?;
        Ok(())
    }

    fn write(&self, relative: &str, lines: &[Value]) -> TestResult<()> {
        let path = self.home().join(relative);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text: Vec<String> = lines.iter().map(Value::to_string).collect();
        std::fs::write(path, text.join("\n"))?;
        Ok(())
    }

    /// A Claude Code session on branch `KD-214-checkout-loop`: 1M input,
    /// 100k output and 1M cache reads on Sonnet 4.5, $4.80 at list price.
    fn claude_code_session(&self) -> TestResult<()> {
        let cwd = self.work("storefront").display().to_string();
        let branch = "KD-214-checkout-loop";
        let prompt = json!({"type": "user", "sessionId": "cc-1", "timestamp": "2026-10-11T09:00:00Z", "cwd": cwd, "gitBranch": branch,
            "message": {"role": "user", "content": "Fix the checkout redirect loop. The follow-up will be KD-260."}});
        let first = json!({"type": "assistant", "sessionId": "cc-1", "timestamp": "2026-10-11T09:01:00Z", "cwd": cwd, "gitBranch": branch, "requestId": "req_1",
            "message": {"id": "msg_1", "model": "claude-sonnet-4-5", "usage": {"input_tokens": 1_000_000, "output_tokens": 100_000}}});
        let second = json!({"type": "assistant", "sessionId": "cc-1", "timestamp": "2026-10-11T09:20:00Z", "cwd": cwd, "gitBranch": branch, "requestId": "req_2",
            "message": {"id": "msg_2", "model": "claude-sonnet-4-5", "usage": {"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 1_000_000}}});
        let title = json!({"type": "ai-title", "sessionId": "cc-1", "aiTitle": "Fix checkout redirect loop"});
        self.write(
            ".claude/projects/-work-storefront/cc-1.jsonl",
            &[prompt, first.clone(), first, second, title],
        )
    }

    /// A Codex session in the ops repository whose prompt names OPS-7:
    /// $0.64 at list price on GPT-5.
    fn codex_session(&self) -> TestResult<()> {
        let cwd = self.work("ops").display().to_string();
        let usage = json!({"input_tokens": 200_000, "cached_input_tokens": 100_000, "output_tokens": 50_000});
        self.write(
            ".codex/sessions/2026/10/11/rollout-2026-10-11T10-00-00-cx-1.jsonl",
            &[
                json!({"timestamp": "2026-10-11T10:00:00Z", "type": "session_meta", "payload": {"id": "cx-1", "cwd": cwd, "git": {"branch": "main", "repository_url": "https://github.com/kestrel/ops.git"}}}),
                json!({"timestamp": "2026-10-11T10:00:01Z", "type": "turn_context", "payload": {"model": "gpt-5", "cwd": cwd}}),
                json!({"timestamp": "2026-10-11T10:00:02Z", "type": "event_msg", "payload": {"type": "user_message", "message": "Rotate the deploy keys for OPS-7"}}),
                json!({"timestamp": "2026-10-11T10:05:00Z", "type": "event_msg", "payload": {"type": "token_count", "info": {"last_token_usage": usage, "total_token_usage": usage}}}),
                json!({"timestamp": "2026-10-11T10:05:01Z", "type": "event_msg", "payload": {"type": "token_count", "info": {"last_token_usage": usage, "total_token_usage": usage}}}),
            ],
        )
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tc"));
        command
            .args(args)
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("TC_HOME", self.dir.path().join("tc"))
            .env("TZ", "UTC")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("APPDATA")
            .env_remove("TC_URL")
            .env_remove("TC_TOKEN")
            .env_remove("TC_TAGS")
            .env_remove("TC_PROJECT")
            .env_remove("TC_TICKET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)], stdin: &str) -> TestResult<Output> {
        let mut command = self.command(args);
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(stdin.as_bytes())?;
        }
        Ok(child.wait_with_output()?)
    }

    /// Runs tc and expects it to succeed.
    fn tc(&self, args: &[&str]) -> TestResult<String> {
        self.tc_with(args, &[], "")
    }

    fn tc_with(&self, args: &[&str], env: &[(&str, &str)], stdin: &str) -> TestResult<String> {
        let output = self.run(args, env, stdin)?;
        let stdout = String::from_utf8(output.stdout)?;
        if !output.status.success() {
            return Err(format!(
                "tc {} failed: {}{stdout}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(stdout)
    }

    fn json(&self, args: &[&str]) -> TestResult<Value> {
        Ok(serde_json::from_str(&self.tc(args)?)?)
    }

    /// A machine with both sessions, a ticket key pattern and no service.
    fn ready() -> TestResult<Self> {
        let machine = Self::new()?;
        machine.claude_code_session()?;
        machine.codex_session()?;
        machine.tc(&["config", "since", "2026-10-01"])?;
        machine.tc(&["config", "ticket-key-pattern", r"^(KD|OPS)-\d+"])?;
        Ok(machine)
    }
}

/// A request the fake service received.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Value,
}

type Answer = Box<dyn Fn(&Seen) -> (u16, Value) + Send + Sync>;

/// A fake service on a free local port. It answers every request with
/// `answer` and keeps what it saw.
struct Service {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Service {
    fn start(answer: Answer) -> TestResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // A broken request fails the test through what tc prints.
                let _ignored = handle(stream, &answer, &kept);
            }
        });
        Ok(Self { url, seen })
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    fn paths(&self) -> Vec<String> {
        self.seen()
            .into_iter()
            .map(|seen| format!("{} {}", seen.method, seen.path))
            .collect()
    }
}

fn handle(stream: TcpStream, answer: &Answer, seen: &Mutex<Vec<Seen>>) -> TestResult<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut length = 0_usize;
    let mut authorization = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse()?,
                "authorization" => authorization = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    let request = Seen {
        method,
        path,
        authorization,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    };
    let (status, reply) = answer(&request);
    if let Ok(mut list) = seen.lock() {
        list.push(request);
    }
    let text = reply.to_string();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()?;
    Ok(())
}

fn context() -> Value {
    json!({
        "organization": {
            "name": "Kestrel Digital",
            "currency": "EUR",
            "tag_names": ["team", "purpose", "client"],
            "rules": [
                {"name": "ticket key in branch", "then": {"ticket_key_in": "branch"}},
                {"name": "ticket key in pull request title", "then": {"ticket_key_in": "pull_request_title"}},
                {"name": "ticket key in prompts or session title", "then": {"ticket_key_in": "prompts_or_session_title"}},
                {"name": "repository", "then": "repository_project"},
                {"name": "ops work", "when": [{"signal": "repository", "test": {"is": "kestrel/ops"}}], "then": {"set_tags": {"purpose": "operations"}}}
            ]
        },
        "person": {"email": "m.ortner@kestrel.example", "tags": {"team": "web"}},
        "projects": [
            {"name": "storefront", "ticket_key_pattern": "^KD-\\d+", "repositories": ["kestrel/storefront"], "tags": {"client": "acme"}},
            {"name": "ops", "ticket_key_pattern": "^OPS-\\d+", "repositories": ["kestrel/ops"]}
        ],
        "rules": [],
        "cadence": {"period_start": "2026-10-05", "period_end": "2026-10-11", "due_at": "2026-10-13T09:00:00Z"},
        "rejected": []
    })
}

/// The review list's entries by agent harness.
fn by_harness(review: &Value) -> BTreeMap<String, Value> {
    review["token_entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry["agent_harness"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        entry.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn drafts_come_from_both_agents_with_no_service_connected() {
    let machine = Machine::ready().unwrap();
    let review = machine.json(&["review", "--json"]).unwrap();
    let entries = by_harness(&review);
    assert_eq!(entries.len(), 2);

    let claude = &entries["claude-code"];
    assert_eq!(claude["session_id"], "cc-1");
    assert_eq!(claude["model"], "claude-sonnet-4-5");
    // The repeated line of msg_1 counts once.
    assert_eq!(claude["tokens"]["input"], 1_000_000);
    assert_eq!(claude["tokens"]["cache_read"], 1_000_000);
    assert_eq!(claude["list_value"], "4.80");
    assert_eq!(claude["currency"], "USD");
    assert_eq!(claude["splits"][0]["tags"]["ticket"], "KD-214");
    assert_eq!(claude["placed_by"], "rule: ticket key in branch");
    assert_eq!(claude["candidates"], json!(["KD-260"]));
    assert_eq!(claude["summary"], "Fix checkout redirect loop");
    assert_eq!(claude["started_at"], "2026-10-11T09:01:00Z");

    let codex = &entries["codex"];
    assert_eq!(codex["model"], "gpt-5");
    assert_eq!(codex["tokens"]["input"], 100_000);
    assert_eq!(codex["tokens"]["cache_read"], 100_000);
    assert_eq!(codex["list_value"], "0.64");
    assert_eq!(codex["splits"][0]["tags"]["ticket"], "OPS-7");

    // Reading again changes nothing.
    assert_eq!(machine.json(&["review", "--json"]).unwrap(), review);

    let table = machine.tc(&["review"]).unwrap();
    assert!(table.contains("$4.80"), "{table}");
    assert!(table.contains("ticket KD-214"), "{table}");
    assert!(table.contains("Fix checkout redirect loop"), "{table}");
}

#[test]
fn people_tag_split_and_export_by_hand() {
    let machine = Machine::ready().unwrap();
    let review = machine.json(&["review", "--json"]).unwrap();
    let number = |harness: &str| by_harness(&review)[harness]["number"].to_string();
    let claude = number("claude-code");
    let codex = number("codex");

    let tagged = machine
        .json(&["tag", &codex, "purpose=learning", "ticket=OPS-9", "--json"])
        .unwrap();
    assert_eq!(
        tagged["token_entry"]["splits"][0]["tags"]["ticket"],
        "OPS-9"
    );
    assert_eq!(
        tagged["token_entry"]["splits"][0]["tags"]["purpose"],
        "learning"
    );
    assert_eq!(tagged["token_entry"]["placed_by"], "hand");

    let split = machine
        .json(&["split", &claude, "KD-214=60", "KD-260=40", "--json"])
        .unwrap();
    let splits = split["token_entry"]["splits"].as_array().unwrap();
    assert_eq!(splits.len(), 2);
    assert_eq!(splits[0]["share_percent"], 60);
    assert_eq!(splits[0]["list_value"], "2.88");
    assert_eq!(splits[1]["tags"]["ticket"], "KD-260");
    assert_eq!(splits[1]["list_value"], "1.92");

    let csv = machine.tc(&["export", "--csv"]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 4, "{csv}");
    assert!(lines[0].starts_with("number,status,agent_harness,session_id,model"));
    assert!(lines[0].contains(",purpose,"), "{}", lines[0]);
    assert!(csv.contains("KD-260"), "{csv}");
    assert!(csv.contains(",learning,"), "{csv}");

    let report = machine
        .json(&[
            "report",
            "--by",
            "ticket",
            "--since",
            "2026-10-01",
            "--json",
        ])
        .unwrap();
    assert_eq!(report["total"], "5.44");
    assert_eq!(report["rows"][0]["value"], "KD-214");
    assert_eq!(report["rows"][0]["list_value"], "2.88");

    let day = machine
        .json(&["report", "--by", "day", "--since", "2026-10-01", "--json"])
        .unwrap();
    assert_eq!(day["rows"][0]["value"], "2026-10-11");
    assert_eq!(day["rows"][0]["list_value"], "5.44");
}

#[test]
fn the_mod_records_summaries_and_tags_line_by_line() {
    let machine = Machine::ready().unwrap();
    let lines = [
        json!({"kind": "turn", "session_id": "cc-1", "turn_id": "t1", "at": "2026-10-11T09:01:00Z", "model": "claude-sonnet-4-5", "tokens": {"input": 5, "output": 5, "cache_write": 0, "cache_read": 0}, "reported_cost": 0.5, "cwd": "/x", "branch": null, "repository": null, "ticket_keys": []}),
        json!({"kind": "summary", "session_id": "cc-1", "summary": "Fixed the redirect loop in checkout"}),
        json!({"kind": "tags", "session_id": "cc-1", "tags": {"ticket": "KD-260"}}),
        json!({"kind": "tags", "session_id": "no-such-session", "tags": {"ticket": "KD-1"}}),
    ];
    let input: Vec<String> = lines.iter().map(Value::to_string).collect();
    let recorded: Value = serde_json::from_str(
        &machine
            .tc_with(&["record", "--json"], &[], &input.join("\n"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(recorded["recorded"], 4);

    let review = machine.json(&["review", "--json"]).unwrap();
    let claude = &by_harness(&review)["claude-code"];
    assert_eq!(claude["summary"], "Fixed the redirect loop in checkout");
    assert_eq!(claude["splits"][0]["tags"]["ticket"], "KD-260");
    assert_eq!(claude["placed_by"], "hand");
    // Turn lines are not counted twice: the session file has the tokens.
    assert_eq!(claude["tokens"]["output"], 100_000);
}

#[test]
fn rules_are_added_listed_and_removed_on_the_machine() {
    let machine = Machine::ready().unwrap();
    machine
        .tc(&[
            "rules",
            "add",
            "directory",
            "contains",
            "ops",
            "purpose=operations",
        ])
        .unwrap();
    machine
        .tc(&["rules", "add", "branch ^review/ → purpose=code-review"])
        .unwrap();
    let rules = machine.json(&["rules", "--json"]).unwrap();
    assert_eq!(rules["rules"][0]["number"], 1);
    assert_eq!(
        rules["rules"][0]["text"],
        "directory contains ops → purpose=operations"
    );
    assert_eq!(rules["rules"][1]["when"][0]["test"]["matches"], "^review/");
    assert_eq!(
        rules["organization"][0]["text"], "ticket key in branch",
        "{rules}"
    );

    let review = machine.json(&["review", "--json"]).unwrap();
    assert_eq!(
        by_harness(&review)["codex"]["splits"][0]["tags"]["purpose"],
        "operations"
    );

    machine.tc(&["rules", "move", "2", "1"]).unwrap();
    machine.tc(&["rules", "remove", "2"]).unwrap();
    let rules = machine.json(&["rules", "--json"]).unwrap();
    assert_eq!(rules["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        rules["rules"][0]["then"]["set_tags"]["purpose"],
        "code-review"
    );

    let wrong = machine
        .run(&["rules", "add", "weather", "is", "sunny", "x=y"], &[], "")
        .unwrap();
    assert_eq!(wrong.status.code(), Some(1));
}

#[test]
fn mistakes_exit_with_one_and_say_what_to_do() {
    let machine = Machine::ready().unwrap();
    let submit = machine.run(&["submit"], &[], "").unwrap();
    assert_eq!(submit.status.code(), Some(1));
    let said = String::from_utf8(submit.stderr).unwrap();
    assert!(said.contains("tc connect URL"), "{said}");
    assert!(said.contains("tc export --csv"), "{said}");

    let missing = machine.run(&["tag", "99", "ticket=KD-1"], &[], "").unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let usage = machine.run(&["report"], &[], "").unwrap();
    assert_eq!(usage.status.code(), Some(1));

    let version = machine.tc(&["--version"]).unwrap();
    assert_eq!(version.trim(), "tc 1.0.0");

    let status = machine.json(&["status", "--json"]).unwrap();
    assert_eq!(status["connected"], false);
    assert_eq!(status["today"]["currency"], "USD");
    assert_eq!(status["drafts"], 2);
    // With no service there is no cadence and so no token sheet period.
    assert_eq!(status["token_sheet"], Value::Null);
}

#[test]
fn a_token_sheet_is_submitted_with_keys_but_never_prompts() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => (200, context()),
        "/api/v1/token-sheets/submit" => {
            let answered: Vec<Value> = seen.body["token_entries"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| json!({"local_id": entry["local_id"], "id": "te_1", "status": "submitted"}))
                        .collect()
                })
                .unwrap_or_default();
            (200, json!({"token_entries": answered}))
        }
        _ => (404, json!({"error": "no such route"})),
    }))
    .unwrap();
    let env = [
        ("TC_URL", service.url.as_str()),
        ("TC_TOKEN", "tc_ses_test"),
    ];

    let review: Value =
        serde_json::from_str(&machine.tc_with(&["review", "--json"], &env, "").unwrap()).unwrap();
    let codex = &by_harness(&review)["codex"];
    // The organization rule and the project's own tags apply.
    assert_eq!(codex["splits"][0]["tags"]["purpose"], "operations");
    assert_eq!(codex["splits"][0]["tags"]["project"], "ops");

    let said = machine.tc_with(&["submit"], &env, "").unwrap();
    assert!(
        said.starts_with("submitted token sheet · 2 token entries"),
        "{said}"
    );

    let seen = service.seen();
    let submit = seen
        .iter()
        .find(|seen| seen.path == "/api/v1/token-sheets/submit")
        .unwrap();
    assert_eq!(submit.authorization.as_deref(), Some("Bearer tc_ses_test"));
    let sent = submit.body.to_string();
    assert!(!sent.contains("redirect loop. The follow-up"), "{sent}");
    assert!(!sent.contains("Rotate the deploy keys"), "{sent}");
    let entries = submit.body["token_entries"].as_array().unwrap();
    let claude = entries
        .iter()
        .find(|entry| entry["agent_harness"] == "claude-code")
        .unwrap();
    assert_eq!(claude["local_id"], "claude-code:cc-1");
    assert_eq!(claude["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(claude["signals"]["branch"], "KD-214-checkout-loop");
    assert_eq!(claude["signals"]["repository"], "kestrel/storefront");
    assert_eq!(claude["signals"]["ticket_keys"], json!(["KD-260"]));
    assert_eq!(claude["splits"][0]["tags"]["ticket"], "KD-214");
    assert_eq!(claude["placed_by"], "rule: ticket key in branch");

    // Submitted entries leave the review list. The next command reads the
    // context again, since the service's word on late sheets changed.
    let context_reads = |service: &Service| {
        service
            .paths()
            .iter()
            .filter(|path| path.as_str() == "GET /api/v1/me/context")
            .count()
    };
    let before = context_reads(&service);
    let review: Value =
        serde_json::from_str(&machine.tc_with(&["review", "--json"], &env, "").unwrap()).unwrap();
    assert_eq!(review["token_entries"].as_array().map(Vec::len), Some(0));
    assert_eq!(context_reads(&service), before + 1);
    // A second submit sends an empty token sheet: nothing to report, so
    // the sheet is not late.
    let again = machine.tc_with(&["submit"], &env, "").unwrap();
    assert_eq!(again.trim(), "submitted token sheet · nothing to report");
    let seen = service.seen();
    let submits: Vec<&Seen> = seen
        .iter()
        .filter(|seen| seen.path == "/api/v1/token-sheets/submit")
        .collect();
    assert_eq!(submits.len(), 2);
    assert_eq!(
        submits.last().map(|seen| &seen.body),
        Some(&json!({"token_entries": []}))
    );
}

#[test]
fn with_no_drafts_and_no_service_there_is_nothing_to_submit() {
    let machine = Machine::new().unwrap();
    let said = machine.tc(&["submit"]).unwrap();
    assert!(said.starts_with("Nothing to submit"), "{said}");
}

#[test]
fn the_github_action_reports_automatic_entries_with_its_tags_and_signals() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => (200, context()),
        "/api/v1/token-entries" => (201, json!({"id": "te_9"})),
        _ => (404, json!({"error": "no such route"})),
    }))
    .unwrap();
    // Spelled as the Action spells it.
    let signals = json!({"repository": "github.com/kestrel/ops", "pull_request": 12, "pull_request_title": "KD-300: tidy the cart"}).to_string();
    let env = [
        ("TC_URL", service.url.as_str()),
        ("TC_TOKEN", "tc_svc_ci"),
        ("TC_TAGS", "purpose=code-review"),
        ("TC_SIGNALS", signals.as_str()),
    ];
    let said: Value =
        serde_json::from_str(&machine.tc_with(&["submit", "--json"], &env, "").unwrap()).unwrap();
    assert_eq!(said["submitted"], 2);
    assert_eq!(said["list_value"], "5.44");
    assert_eq!(said["currency"], "USD");
    let posted: Vec<Seen> = service
        .seen()
        .into_iter()
        .filter(|seen| seen.path == "/api/v1/token-entries")
        .collect();
    assert_eq!(posted.len(), 2);
    let codex = posted
        .iter()
        .find(|seen| seen.body["agent_harness"] == "codex")
        .unwrap();
    assert_eq!(codex.body["tags"]["purpose"], "code-review");
    // The service takes the pull request as a number.
    assert_eq!(codex.body["signals"]["pull_request"], 12);
    assert_eq!(codex.body["signals"]["repository"], "kestrel/ops");
    assert_eq!(
        codex.body["signals"]["ticket_keys"],
        json!(["OPS-7", "KD-300"])
    );
    // A ticket in the pull request title wins over one in the prompts.
    assert_eq!(codex.body["splits"][0]["tags"]["ticket"], "KD-300");

    let wrong = machine
        .run(&["submit", "--signal", "weather=sunny"], &env, "")
        .unwrap();
    assert_eq!(wrong.status.code(), Some(1));

    // The same names as flags, for scripts.
    let flagged = Machine::ready().unwrap();
    let said = flagged
        .tc_with(
            &[
                "submit",
                "--tag",
                "purpose=learning",
                "--signal",
                "pull_request=7",
            ],
            &env[..2],
            "",
        )
        .unwrap();
    assert!(said.starts_with("reported · 2 token entries"), "{said}");
}

#[test]
fn connect_signs_in_with_a_magic_link_and_keeps_the_token() {
    let machine = Machine::ready().unwrap();
    let polls = Arc::new(Mutex::new(0_u32));
    let counter = Arc::clone(&polls);
    let service = Service::start(Box::new(move |seen: &Seen| {
        match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/api/v1/invites/kestrel-4f2a") => (
                200,
                json!({"organization_name": "Kestrel Digital", "email": "m.ortner@kestrel.example"}),
            ),
            ("POST", "/api/v1/sign-in") => (202, json!({"poll_token": "p0ll"})),
            ("POST", "/api/v1/sign-in/poll") => {
                let mut count = counter.lock().map(|count| *count).unwrap_or_default();
                count = count.saturating_add(1);
                if let Ok(mut stored) = counter.lock() {
                    *stored = count;
                }
                if count < 2 {
                    (202, json!({"status": "waiting"}))
                } else {
                    (
                        200,
                        json!({"status": "signed_in", "session_token": "tc_ses_new"}),
                    )
                }
            }
            ("GET", "/api/v1/me") => (
                200,
                json!({"person": {"email": "m.ortner@kestrel.example", "name": "Mara Ortner"}, "organization": {"name": "Kestrel Digital", "slug": "kestrel"}, "credential": "session"}),
            ),
            ("GET", "/api/v1/me/context") => (200, context()),
            _ => (404, json!({"error": "not_found", "message": "not found"})),
        }
    }))
    .unwrap();
    let invite = format!("{}/i/kestrel-4f2a", service.url);
    let output = machine.run(&["connect", &invite], &[], "").unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}{stdout}");
    assert!(
        stderr.contains("Sent a sign-in link to m.ortner@kestrel.example"),
        "{stderr}"
    );
    assert!(stdout.contains("Kestrel Digital"), "{stdout}");

    let seen = service.seen();
    let sign_in = seen
        .iter()
        .find(|seen| seen.path == "/api/v1/sign-in")
        .unwrap();
    assert_eq!(sign_in.body["email"], "m.ortner@kestrel.example");
    assert_eq!(sign_in.body["local_tool"], true);
    let polled = seen
        .iter()
        .filter(|seen| seen.path == "/api/v1/sign-in/poll")
        .count();
    assert_eq!(polled, 2);
    let me = seen.iter().find(|seen| seen.path == "/api/v1/me").unwrap();
    assert_eq!(me.authorization.as_deref(), Some("Bearer tc_ses_new"));
    assert!(
        service
            .paths()
            .contains(&"GET /api/v1/me/context".to_owned())
    );

    let config =
        std::fs::read_to_string(machine.dir.path().join("tc").join("config.toml")).unwrap();
    assert!(config.contains("token = \"tc_ses_new\""), "{config}");
    assert!(
        config.contains(&format!("service = \"{}\"", service.url)),
        "{config}"
    );

    let status = machine.json(&["status", "--json"]).unwrap();
    assert_eq!(status["connected"], true);
    assert_eq!(status["organization"], "Kestrel Digital");
    // The organization's own rule, not the built-in ones.
    assert_eq!(status["rules"], 1);
    assert_eq!(status["token_sheet"]["period_start"], "2026-10-05");
    assert_eq!(status["token_sheet"]["late"], false);
}

/// The service says when a token sheet is late and when the controller
/// asked everyone to submit before the close; `tc status` passes both on.
#[test]
fn status_shows_a_late_sheet_and_a_request_to_submit() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => {
            let mut context = context();
            context["cadence"]["late"] = json!(true);
            context["asked_to_submit"] =
                json!({"period": "2026-09", "asked_by": "Lena Hofer", "asked_at": "2026-10-02T08:00:00Z"});
            (200, context)
        }
        _ => (404, json!({"error": "no such route"})),
    }))
    .unwrap();
    let env = [
        ("TC_URL", service.url.as_str()),
        ("TC_TOKEN", "tc_cli_test"),
    ];
    let status: Value =
        serde_json::from_str(&machine.tc_with(&["status", "--json"], &env, "").unwrap()).unwrap();
    assert_eq!(status["token_sheet"]["late"], true);
    assert_eq!(status["asked_to_submit"]["period"], "2026-09");
    let said = machine.tc_with(&["status"], &env, "").unwrap();
    assert!(said.contains("· late: submit with tc submit"), "{said}");
    assert!(
        said.contains("asked by Lena Hofer to submit for 2026-09"),
        "{said}"
    );
}

/// Without an invite, `tc connect` asks for the address, and says so when
/// the link runs out.
#[test]
fn connect_to_an_instance_asks_for_the_email_address() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| {
        match (seen.method.as_str(), seen.path.as_str()) {
            ("POST", "/api/v1/sign-in") => (202, json!({"poll_token": "p0ll"})),
            ("POST", "/api/v1/sign-in/poll") => (
                410,
                json!({"error": "gone", "message": "The sign-in link expired after 15 minutes. Sign in again."}),
            ),
            _ => (404, json!({"error": "not_found", "message": "not found"})),
        }
    }))
    .unwrap();
    let output = machine
        .run(&["connect", &service.url], &[], "jan@example.com\n")
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("Your email address at"), "{stderr}");
    assert!(stderr.contains("expired after 15 minutes"), "{stderr}");
    let seen = service.seen();
    assert_eq!(seen[0].body["email"], "jan@example.com");

    let nothing = machine.run(&["connect", &service.url], &[], "").unwrap();
    assert_eq!(nothing.status.code(), Some(1));
    let config = machine.dir.path().join("tc").join("config.toml");
    let saved = std::fs::read_to_string(config).unwrap_or_default();
    assert!(!saved.contains("token"), "{saved}");
}

/// The sessions folder of an agent can be moved with its own variable.
#[test]
fn agents_folders_follow_their_own_variables() {
    let machine = Machine::new().unwrap();
    machine.claude_code_session().unwrap();
    machine.codex_session().unwrap();
    machine.tc(&["config", "since", "2026-10-01"]).unwrap();
    let elsewhere: &Path = &machine.dir.path().join("nowhere");
    let output = machine
        .tc_with(
            &["review", "--json"],
            &[
                ("CLAUDE_CONFIG_DIR", &elsewhere.display().to_string()),
                ("CODEX_HOME", &elsewhere.display().to_string()),
            ],
            "",
        )
        .unwrap();
    let review: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(review["token_entries"].as_array().map(Vec::len), Some(0));
}

#[cfg(unix)]
#[test]
fn the_classifier_reads_the_prompts_that_name_a_ticket_and_has_the_last_word() {
    let machine = Machine::ready().unwrap();
    let asked = machine.dir.path().join("asked.json");
    let program = format!("cat > '{}'; echo none", asked.display());
    machine.tc(&["config", "classifier", &program]).unwrap();

    let review = machine.json(&["review", "--json"]).unwrap();
    let entries = by_harness(&review);
    // Only the Codex session's ticket came from a prompt alone.
    assert_eq!(entries["codex"]["splits"][0]["tags"].get("ticket"), None);
    assert_eq!(entries["codex"]["placed_by"], "none");
    assert_eq!(entries["codex"]["candidates"], json!(["OPS-7"]));
    assert_eq!(
        entries["claude-code"]["splits"][0]["tags"]["ticket"],
        "KD-214"
    );

    let digest: Value = serde_json::from_str(&std::fs::read_to_string(&asked).unwrap()).unwrap();
    assert_eq!(digest["agent_harness"], "codex");
    assert_eq!(digest["repository"], "kestrel/ops");
    assert_eq!(digest["candidates"][0]["ticket"], "OPS-7");
    assert_eq!(
        digest["candidates"][0]["mentions"],
        json!(["Rotate the deploy keys for OPS-7"])
    );

    // The reference script turns the same digest into one question for Laya.
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("classifier/pick-ticket");
    let mut child = Command::new("python3")
        .arg(&script)
        .arg("--question")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(digest.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let question: Value = serde_json::from_slice(&output.stdout).unwrap();
    let criteria = &question["questions"]["ticket"]["criteria"];
    assert!(criteria.get("OPS-7").is_some());
    assert!(criteria.get("none").is_some());
    assert!(
        question["state"]
            .as_str()
            .unwrap()
            .contains("Rotate the deploy keys for OPS-7")
    );
}
