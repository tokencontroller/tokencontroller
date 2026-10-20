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

/// The person's own price table, in US dollars per million tokens.
const PRICES: &str = "provider,model,kind,price_per_million,valid_from
anthropic,claude-sonnet-4-5,input,3,2026-01-01
anthropic,claude-sonnet-4-5,output,15,2026-01-01
anthropic,claude-sonnet-4-5,cache_write,3.75,2026-01-01
anthropic,claude-sonnet-4-5,cache_read,0.3,2026-01-01
openai,gpt-5,input,1.25,2026-01-01
openai,gpt-5,output,10,2026-01-01
openai,gpt-5,cache_write,1.25,2026-01-01
openai,gpt-5,cache_read,0.125,2026-01-01
";

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
    /// 100k output and 1M cache reads on Sonnet 4.5, $4.80 at the prices
    /// above.
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
    /// $0.64 on GPT-5 at the prices above.
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

    /// A Claude Code session with one model call, run in `cwd`, which is
    /// created.
    fn claude_code_session_in(&self, session: &str, cwd: &Path) -> TestResult<()> {
        std::fs::create_dir_all(cwd)?;
        let cwd = cwd.display().to_string();
        let call = json!({"type": "assistant", "sessionId": session, "timestamp": "2026-10-11T11:00:00Z", "cwd": cwd, "gitBranch": "main", "requestId": format!("req_{session}"),
            "message": {"id": format!("msg_{session}"), "model": "claude-sonnet-4-5", "usage": {"input_tokens": 1000, "output_tokens": 100}}});
        self.write(
            &format!(".claude/projects/-work-x/{session}.jsonl"),
            &[call],
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
            .env_remove("TC_FEATURE")
            .env_remove("TC_SIGNALS")
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

    /// A machine with both sessions, a feature key pattern, the person's own
    /// price file and no service.
    fn ready() -> TestResult<Self> {
        let machine = Self::unpriced()?;
        let prices = machine.dir.path().join("prices.csv");
        std::fs::write(&prices, PRICES)?;
        machine.tc(&["config", "price-table", &prices.display().to_string()])?;
        Ok(machine)
    }

    /// [`Self::ready`] without a price file: no cost, never zero.
    fn unpriced() -> TestResult<Self> {
        let machine = Self::new()?;
        machine.claude_code_session()?;
        machine.codex_session()?;
        machine.tc(&["config", "since", "2026-10-01"])?;
        machine.tc(&["config", "feature-key-pattern", r"^(KD|OPS)-\d+"])?;
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

/// The organization's context: euros, two features on record with their
/// project, one rule that finds a feature key in the branch and one that
/// puts the ops repository on a purpose, and a price table in euros.
fn context() -> Value {
    json!({
        "organization": {"name": "Kestrel Digital", "currency": "EUR"},
        "person": {"email": "m.ortner@kestrel.example"},
        "tag_names": ["team", "purpose", "client"],
        "tag_values": [
            {"name": "feature", "key": "KD-214", "defaults": {"project": "storefront"}},
            {"name": "feature", "key": "KD-300", "defaults": {"project": "storefront"}},
            {"name": "feature", "key": "OPS-7", "defaults": {"project": "ops"}},
            {"name": "project", "key": "storefront", "defaults": {"client": "acme"}},
            {"name": "project", "key": "ops"}
        ],
        "attribute_values": [{"name": "team", "key": "web"}],
        "attributes": [{"person": "me", "name": "team", "key": "web"}],
        "rules": [
            {"name": "key in branch", "scope": "organization", "order": 0, "signal": "branch",
             "pattern": "(?:^|[^A-Za-z0-9_])(?P<key>(KD|OPS)-\\d+)", "then": {"key_of": {"name": "feature"}}},
            {"name": "key in PR title", "scope": "organization", "order": 1, "signal": "pull_request_title",
             "pattern": "(?:^|[^A-Za-z0-9_])(?P<key>(KD|OPS)-\\d+)", "then": {"key_of": {"name": "feature"}}},
            {"name": "ops work", "scope": "organization", "order": 2, "signal": "repository",
             "pattern": "^kestrel/ops$", "then": {"tag": {"name": "purpose", "key": "operations"}}}
        ],
        "my_rules": [],
        "price_table": [
            {"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "input", "per_million": "3", "valid_from": "2026-01-01"},
            {"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "output", "per_million": "15", "valid_from": "2026-01-01"},
            {"provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "cache_read", "per_million": "0.3", "valid_from": "2026-01-01"},
            {"provider": "openai", "model": "gpt-5", "kind": "input", "per_million": "1.25", "valid_from": "2026-01-01"},
            {"provider": "openai", "model": "gpt-5", "kind": "output", "per_million": "10", "valid_from": "2026-01-01"},
            {"provider": "openai", "model": "gpt-5", "kind": "cache_read", "per_million": "0.125", "valid_from": "2026-01-01"}
        ],
        "cutoff": "2026-09-30",
        "submissions": [],
        "asked_to_submit": null
    })
}

/// The review list's sessions by agent harness.
fn by_harness(review: &Value) -> BTreeMap<String, Value> {
    review["sessions"]
        .as_array()
        .map(|sessions| {
            sessions
                .iter()
                .map(|session| {
                    (
                        session["agent_harness"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        session.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn sessions_come_from_both_agents_with_no_service_connected() {
    let machine = Machine::ready().unwrap();
    let review = machine.json(&["review", "--json"]).unwrap();
    let sessions = by_harness(&review);
    assert_eq!(sessions.len(), 2);
    assert_eq!(review["currency"], "USD");

    let claude = &sessions["claude-code"];
    assert_eq!(claude["session_id"], "cc-1");
    assert_eq!(claude["model"], "claude-sonnet-4-5");
    assert_eq!(claude["provider"], "anthropic");
    // The repeated line of msg_1 counts once.
    assert_eq!(claude["tokens"]["input"], 1_000_000);
    assert_eq!(claude["tokens"]["cache_read"], 1_000_000);
    assert_eq!(claude["cost"], "4.80");
    assert_eq!(claude["splits"][0]["tags"]["feature"], "KD-214");
    assert_eq!(claude["splits"][0]["fraction"], "1");
    assert_eq!(claude["assigned_by"], "rule: key in branch");
    assert_eq!(claude["label"], "direct");
    assert_eq!(claude["status"], "open");
    assert_eq!(claude["keys"], json!(["KD-260"]));
    assert_eq!(claude["summary"], "Fix checkout redirect loop");
    assert_eq!(claude["started_at"], "2026-10-11T09:01:00Z");
    assert_eq!(claude["messages"], 2);

    let codex = &sessions["codex"];
    assert_eq!(codex["model"], "gpt-5");
    assert_eq!(codex["tokens"]["input"], 100_000);
    assert_eq!(codex["tokens"]["cache_read"], 100_000);
    assert_eq!(codex["cost"], "0.64");
    assert_eq!(codex["splits"][0]["tags"]["feature"], "OPS-7");
    assert_eq!(codex["assigned_by"], "rule: key in prompts");

    // Reading again changes nothing.
    assert_eq!(machine.json(&["review", "--json"]).unwrap(), review);

    let table = machine.tc(&["review"]).unwrap();
    assert!(table.contains("$4.80"), "{table}");
    assert!(table.contains("feature KD-214"), "{table}");
    assert!(table.contains("rule: key in branch"), "{table}");
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
        .json(&["tag", &codex, "purpose=learning", "feature=OPS-9", "--json"])
        .unwrap();
    assert_eq!(tagged["session"]["splits"][0]["tags"]["feature"], "OPS-9");
    assert_eq!(
        tagged["session"]["splits"][0]["tags"]["purpose"],
        "learning"
    );
    assert_eq!(tagged["session"]["assigned_by"], "hand");

    let split = machine
        .json(&["split", &claude, "KD-214=60", "KD-260=40", "--json"])
        .unwrap();
    let splits = split["session"]["splits"].as_array().unwrap();
    assert_eq!(splits.len(), 2);
    assert_eq!(splits[0]["fraction"], "0.6");
    assert_eq!(splits[0]["cost"], "2.88");
    assert_eq!(splits[1]["tags"]["feature"], "KD-260");
    assert_eq!(splits[1]["cost"], "1.92");
    assert_eq!(split["session"]["assigned_by"], "hand");

    // The last split on a session wins; removing a tag keeps the rest.
    let untagged = machine
        .json(&["tag", &codex, "purpose=", "--json"])
        .unwrap();
    assert_eq!(
        untagged["session"]["splits"][0]["tags"].get("purpose"),
        None
    );
    assert_eq!(untagged["session"]["splits"][0]["tags"]["feature"], "OPS-9");

    let csv = machine.tc(&["export", "--csv"]).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    // Two parts of each Claude Code message, one of the Codex message.
    assert_eq!(lines.len(), 6, "{csv}");
    assert!(
        lines[0].starts_with("agent_harness,model,started_at,ended_at,tokens.input,tokens.output,tokens.cache_read,tokens.cache_write,summary,feature,"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].contains(",fraction,cost,currency,label,"),
        "{}",
        lines[0]
    );
    assert!(csv.contains("KD-260"), "{csv}");
    assert!(csv.contains(",0.4,"), "{csv}");

    let report = machine
        .json(&["report", "--since", "2026-10-01", "--json"])
        .unwrap();
    assert_eq!(report["by"], "feature");
    assert_eq!(report["total"], "5.44");
    assert_eq!(report["rows"][0]["value"], "KD-214");
    assert_eq!(report["rows"][0]["cost"], "2.88");

    let day = machine
        .json(&["report", "--by", "day", "--since", "2026-10-01", "--json"])
        .unwrap();
    assert_eq!(day["rows"][0]["value"], "2026-10-11");
    assert_eq!(day["rows"][0]["cost"], "5.44");

    let label = machine
        .json(&["report", "--by", "label", "--since", "2026-10-01", "--json"])
        .unwrap();
    assert_eq!(label["rows"][0]["value"], "direct");
    assert_eq!(label["rows"][0]["cost"], "5.44");
}

#[test]
fn the_mod_records_summaries_facts_and_tags_line_by_line() {
    let machine = Machine::ready().unwrap();
    let lines = [
        json!({"agent_harness": "claude-code", "session_id": "cc-1", "summary": "Fixed the redirect loop in checkout", "facts": {"turns": 4, "duration_seconds": 1200, "subagents": 1}}),
        json!({"agent_harness": "claude-code", "session_id": "cc-1", "feature": "KD-260", "tags": {"purpose": "spike"}}),
        json!({"agent_harness": "claude-code", "session_id": "no-such-session", "feature": "KD-1"}),
    ];
    let input: Vec<String> = lines.iter().map(Value::to_string).collect();
    let recorded: Value = serde_json::from_str(
        &machine
            .tc_with(&["record", "--json"], &[], &input.join("\n"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(recorded["recorded"], 3);
    assert_eq!(recorded["sessions"].as_array().map(Vec::len), Some(1));

    let review = machine.json(&["review", "--json"]).unwrap();
    let claude = &by_harness(&review)["claude-code"];
    assert_eq!(claude["summary"], "Fixed the redirect loop in checkout");
    // A feature picked in the session is the message's own tag; the
    // branch rule still wins for the feature, the other tag stays.
    assert_eq!(claude["splits"][0]["tags"]["feature"], "KD-214");
    assert_eq!(claude["splits"][0]["tags"]["purpose"], "spike");
    // Token counts come from the session file, never from the record.
    assert_eq!(claude["tokens"]["output"], 100_000);
}

#[test]
fn rules_are_added_listed_moved_and_removed_on_the_machine() {
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
    machine
        .tc(&["rules", "add", r"branch matches ^(KD-\d+) feature=$1"])
        .unwrap();
    let rules = machine.json(&["rules", "--json"]).unwrap();
    assert_eq!(rules["rules"][0]["number"], 1);
    assert_eq!(
        rules["rules"][0]["text"],
        "directory contains ops → purpose=operations"
    );
    assert_eq!(rules["rules"][1]["pattern"], "^review/");
    assert_eq!(rules["rules"][2]["then"]["key_of"]["name"], "feature");
    assert_eq!(
        rules["organization"][0]["text"], "key in branch → feature=$1",
        "{rules}"
    );

    let review = machine.json(&["review", "--json"]).unwrap();
    let codex = &by_harness(&review)["codex"];
    assert_eq!(codex["splits"][0]["tags"]["purpose"], "operations");
    assert_eq!(codex["assigned_by"], "rule: directory contains ops");

    machine.tc(&["rules", "move", "2", "1"]).unwrap();
    machine.tc(&["rules", "remove", "2"]).unwrap();
    machine.tc(&["rules", "remove", "2"]).unwrap();
    let rules = machine.json(&["rules", "--json"]).unwrap();
    assert_eq!(rules["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(rules["rules"][0]["then"]["tag"]["key"], "code-review");

    let wrong = machine
        .run(&["rules", "add", "weather", "is", "sunny", "x=y"], &[], "")
        .unwrap();
    assert_eq!(wrong.status.code(), Some(1));
    let gone = machine.run(&["rules", "remove", "5"], &[], "").unwrap();
    assert_eq!(gone.status.code(), Some(1));
}

#[test]
fn remember_turns_a_hand_assignment_into_a_rule() {
    let machine = Machine::ready().unwrap();
    let review = machine.json(&["review", "--json"]).unwrap();
    let codex = by_harness(&review)["codex"]["number"].to_string();
    machine
        .tc(&["tag", &codex, "purpose=operations", "--remember"])
        .unwrap();
    let rules = machine.json(&["rules", "--json"]).unwrap();
    assert_eq!(
        rules["rules"][0]["text"],
        "repository is kestrel/ops → purpose=operations"
    );
    assert_eq!(rules["rules"][0]["signal"], "repository");
    let refused = machine
        .run(
            &["split", &codex, "OPS-7=1", "OPS-8=1", "--remember"],
            &[],
            "",
        )
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
}

#[test]
fn mistakes_exit_with_one_and_say_what_to_do() {
    let machine = Machine::ready().unwrap();
    let submit = machine.run(&["submit"], &[], "").unwrap();
    assert_eq!(submit.status.code(), Some(1));
    let said = String::from_utf8(submit.stderr).unwrap();
    assert!(said.contains("tc connect URL"), "{said}");
    assert!(said.contains("tc export --csv"), "{said}");

    let missing = machine
        .run(&["tag", "99", "feature=KD-1"], &[], "")
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let one = machine.run(&["split", "1", "KD-1=100"], &[], "").unwrap();
    assert_eq!(one.status.code(), Some(1));
    let person = machine.run(&["tag", "1", "person=you"], &[], "").unwrap();
    assert_eq!(person.status.code(), Some(1));
    let usage = machine.run(&["report", "--by", ""], &[], "").unwrap();
    assert_eq!(usage.status.code(), Some(1));
    let setting = machine
        .run(&["config", "since", "yesterday"], &[], "")
        .unwrap();
    assert_eq!(setting.status.code(), Some(1));

    let version = machine.tc(&["--version"]).unwrap();
    assert_eq!(version.trim(), "tc 1.0.0");

    let status = machine.json(&["status", "--json"]).unwrap();
    assert_eq!(status["connected"], false);
    assert_eq!(status["today"]["currency"], "USD");
    assert_eq!(status["open"], 2);
    assert_eq!(status["last_submit"], Value::Null);
    assert_eq!(status["asked_to_submit"], Value::Null);
    let said = machine.tc(&["status"]).unwrap();
    assert!(said.contains("Last submit   none yet"), "{said}");
}

#[test]
fn without_prices_there_is_no_cost_and_never_zero() {
    let machine = Machine::unpriced().unwrap();
    let review = machine.json(&["review", "--json"]).unwrap();
    let claude = &by_harness(&review)["claude-code"];
    assert_eq!(claude["cost"], Value::Null);
    assert_eq!(
        claude["unknown_models"],
        json!(["anthropic/claude-sonnet-4-5"])
    );
    // The split still stands: the rule reads signals, not prices.
    assert_eq!(claude["splits"][0]["tags"]["feature"], "KD-214");
    let table = machine.tc(&["review"]).unwrap();
    assert!(table.contains("no price"), "{table}");
    let report = machine
        .json(&["report", "--since", "2026-10-01", "--json"])
        .unwrap();
    assert_eq!(report["total"], "0.00");
    assert_eq!(report["unknown_models"].as_array().map(Vec::len), Some(2));
    let said = machine.tc(&["report", "--since", "2026-10-01"]).unwrap();
    assert!(said.contains("No price for"), "{said}");
}

#[test]
fn config_lists_the_settings_and_keeps_the_price_file() {
    let machine = Machine::ready().unwrap();
    let all = machine.json(&["config", "--json"]).unwrap();
    let keys: Vec<&str> = all
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "feature-key-pattern",
            "folders",
            "price-table",
            "service",
            "since"
        ]
    );
    assert_eq!(all["since"], "2026-10-01");
    let text = machine.tc(&["config"]).unwrap();
    assert!(text.contains("feature-key-pattern = ^(KD|OPS)"), "{text}");
    let bad = machine.dir.path().join("bad.csv");
    std::fs::write(&bad, "provider,model\n").unwrap();
    let refused = machine
        .run(
            &["config", "price-table", &bad.display().to_string()],
            &[],
            "",
        )
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    let removed = machine.tc(&["config", "price-table", ""]).unwrap();
    assert_eq!(removed.trim(), "price-table removed");
    let review = machine.json(&["review", "--json"]).unwrap();
    assert_eq!(by_harness(&review)["codex"]["cost"], Value::Null);
}

#[test]
fn messages_are_submitted_with_keys_but_never_prompts() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => (200, context()),
        "/api/v1/token-entries" => {
            let answered: Vec<Value> = seen
                .body
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|_| json!({"session_id": "s-1", "messages": 1, "cost": "4.80", "currency": "EUR", "label": "direct", "unknown_tags": []}))
                        .collect()
                })
                .unwrap_or_default();
            (201, Value::Array(answered))
        }
        "/api/v1/submissions" => (201, json!({"id": "sub-1", "status": "submitted"})),
        _ => (404, json!({"error": "no such route"})),
    }))
    .unwrap();
    let env = [
        ("TC_URL", service.url.as_str()),
        ("TC_TOKEN", "tc_ses_test"),
    ];

    let review: Value =
        serde_json::from_str(&machine.tc_with(&["review", "--json"], &env, "").unwrap()).unwrap();
    assert_eq!(review["currency"], "EUR");
    let codex = &by_harness(&review)["codex"];
    // The organization's rule and the feature's defaults apply, and the
    // person's own attributes close the tag set.
    assert_eq!(codex["splits"][0]["tags"]["purpose"], "operations");
    assert_eq!(codex["assigned_by"], "rule: ops work");
    // A purpose alone is neither a feature nor a project.
    assert_eq!(codex["label"], "organization overhead");
    let claude = &by_harness(&review)["claude-code"];
    assert_eq!(claude["splits"][0]["tags"]["feature"], "KD-214");
    assert_eq!(claude["cost"], "4.80");

    let said = machine.tc_with(&["submit"], &env, "").unwrap();
    assert!(said.starts_with("submitted · 2 sessions · €5.44"), "{said}");

    let seen = service.seen();
    let posted = seen
        .iter()
        .find(|seen| seen.path == "/api/v1/token-entries")
        .unwrap();
    assert_eq!(posted.authorization.as_deref(), Some("Bearer tc_ses_test"));
    let sent = posted.body.to_string();
    assert!(!sent.contains("redirect loop. The follow-up"), "{sent}");
    assert!(!sent.contains("Rotate the deploy keys"), "{sent}");
    let entries = posted.body.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let claude = entries
        .iter()
        .find(|entry| entry["agent_harness"] == "claude-code")
        .unwrap();
    assert_eq!(claude["local_id"], "claude-code:cc-1");
    assert_eq!(claude["provider"], "anthropic");
    assert_eq!(claude["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(claude["messages"][1]["tokens"]["cache_read"], 1_000_000);
    assert_eq!(claude["messages"][1]["tokens"]["cache_write"], 0);
    assert_eq!(claude["tokens"]["input"], 1_000_000);
    assert_eq!(claude["signals"]["branch"], "KD-214-checkout-loop");
    assert_eq!(claude["signals"]["repository"], "kestrel/storefront");
    assert_eq!(claude["signals"]["keys"], json!(["KD-260"]));
    assert!(claude["signals"]["working_directory"].is_string());
    assert_eq!(claude.get("splits"), None);
    assert_eq!(claude.get("placed_by"), None);
    let submission = seen
        .iter()
        .find(|seen| seen.path == "/api/v1/submissions")
        .unwrap();
    assert_eq!(submission.body["from"], "2026-10-11");

    // Submitted sessions leave the review list. The next command reads the
    // context again, since the service's submissions changed.
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
    assert_eq!(review["sessions"].as_array().map(Vec::len), Some(0));
    assert_eq!(context_reads(&service), before + 1);
    let all: Value = serde_json::from_str(
        &machine
            .tc_with(&["review", "--all", "--json"], &env, "")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(all["sessions"][0]["status"], "submitted");
    let status: Value =
        serde_json::from_str(&machine.tc_with(&["status", "--json"], &env, "").unwrap()).unwrap();
    assert_eq!(status["last_submit"]["from"], "2026-10-11");
    assert_eq!(status["open"], 0);

    // A second submit tells the service there was nothing to add.
    let again = machine.tc_with(&["submit"], &env, "").unwrap();
    assert_eq!(again.trim(), "submitted · nothing to add");
    let submits = service
        .paths()
        .iter()
        .filter(|path| path.as_str() == "POST /api/v1/submissions")
        .count();
    assert_eq!(submits, 2);
}

#[test]
fn a_rejected_submission_brings_the_sessions_back() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => {
            let mut context = context();
            context["submissions"] = json!([
                {"id": "sub-9", "from": "2026-10-01", "to": "2026-10-31", "status": "rejected", "reason": "wrong client"}
            ]);
            (200, context)
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
    let claude = &by_harness(&review)["claude-code"];
    assert_eq!(claude["status"], "rejected");
    assert_eq!(claude["rejected_reason"], "wrong client");
    let table = machine.tc_with(&["review"], &env, "").unwrap();
    assert!(table.contains("rejected: wrong client"), "{table}");
}

#[test]
fn with_no_open_sessions_and_no_service_there_is_nothing_to_submit() {
    let machine = Machine::new().unwrap();
    let said = machine.tc(&["submit"]).unwrap();
    assert!(said.starts_with("Nothing to submit"), "{said}");
    let json = machine.json(&["submit", "--json"]).unwrap();
    assert_eq!(json["submitted"], 0);
    assert_eq!(json["cost"], "0.00");
}

#[test]
fn the_github_action_reports_with_its_tags_and_signals() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => (200, context()),
        "/api/v1/token-entries" => (
            201,
            json!([{"session_id": "s-9", "unknown_tags": ["purpose"]}, {"session_id": "s-10"}]),
        ),
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
    let output = machine.run(&["submit", "--json"], &env, "").unwrap();
    assert!(output.status.success());
    let said: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(said["submitted"], 2);
    assert_eq!(said["cost"], "5.44");
    assert_eq!(said["currency"], "EUR");
    assert!(said["share_on_features_percent"].is_number());
    let warned = String::from_utf8(output.stderr).unwrap();
    assert!(warned.contains("no tag value yet for purpose"), "{warned}");
    let posted: Vec<Seen> = service
        .seen()
        .into_iter()
        .filter(|seen| seen.path == "/api/v1/token-entries")
        .collect();
    assert_eq!(posted.len(), 1);
    let entries = posted[0].body.as_array().unwrap();
    let codex = entries
        .iter()
        .find(|entry| entry["agent_harness"] == "codex")
        .unwrap();
    assert_eq!(codex["tags"]["purpose"], "code-review");
    // The service takes the pull request as a number.
    assert_eq!(codex["signals"]["pull_request"], 12);
    assert_eq!(
        codex["signals"]["pull_request_title"],
        "KD-300: tidy the cart"
    );
    assert_eq!(codex["signals"]["repository"], "kestrel/ops");
    assert_eq!(codex["signals"]["keys"], json!(["OPS-7", "KD-300"]));
    // A service token reports; it never submits.
    assert!(
        !service
            .paths()
            .contains(&"POST /api/v1/submissions".to_owned())
    );

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
    assert!(said.starts_with("reported · 2 sessions"), "{said}");
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
    assert_eq!(status["rules"], 3);
    assert_eq!(status["cutoff"], "2026-09-30");
    assert_eq!(status["today"]["currency"], "EUR");
}

/// The service lists the person's submissions and an org manager's request
/// to submit before the cutoff moves; `tc status` passes both on.
#[test]
fn status_shows_the_last_submit_and_a_request_to_submit() {
    let machine = Machine::ready().unwrap();
    let service = Service::start(Box::new(|seen: &Seen| match seen.path.as_str() {
        "/api/v1/me/context" => {
            let mut context = context();
            context["submissions"] = json!([
                {"id": "sub-1", "from": "2026-09-01", "to": "2026-09-30", "status": "approved"}
            ]);
            context["asked_to_submit"] = json!({"cutoff": "2026-10-31", "asked_by": "Lena Hofer"});
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
    assert_eq!(status["last_submit"]["to"], "2026-09-30");
    assert_eq!(status["last_submit"]["status"], "approved");
    assert_eq!(status["asked_to_submit"]["cutoff"], "2026-10-31");
    assert_eq!(status["open"], 2);
    let said = machine.tc_with(&["status"], &env, "").unwrap();
    assert!(
        said.contains("Last submit   2026-09-01 to 2026-09-30 · approved"),
        "{said}"
    );
    assert!(
        said.contains("asked by Lena Hofer to submit before the cutoff moves to 2026-10-31"),
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
    assert_eq!(review["sessions"].as_array().map(Vec::len), Some(0));
}

/// The sessions in the review list, sorted.
fn sessions(machine: &Machine) -> TestResult<Vec<String>> {
    let review = machine.json(&["review", "--json"])?;
    let mut sessions: Vec<String> = review
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or("no sessions")?
        .iter()
        .filter_map(|entry| entry.get("session_id")?.as_str().map(str::to_owned))
        .collect();
    sessions.sort();
    Ok(sessions)
}

#[test]
fn tracking_a_folder_counts_only_its_sessions_those_below_and_recorded_ones() {
    let machine = Machine::new().unwrap();
    let client = machine.work("client-a");
    machine.claude_code_session_in("in-a", &client).unwrap();
    machine
        .claude_code_session_in("below-a", &client.join("api"))
        .unwrap();
    machine
        .claude_code_session_in("beside-a", &machine.work("client-a-other"))
        .unwrap();
    machine
        .claude_code_session_in("in-b", &machine.work("client-b"))
        .unwrap();
    machine.tc(&["config", "since", "2026-10-01"]).unwrap();

    // Nothing tracked: every folder counts, and a script is never asked.
    let everything = ["below-a", "beside-a", "in-a", "in-b"];
    let output = machine.run(&["review", "--json"], &[], "").unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().is_empty());
    assert_eq!(sessions(&machine).unwrap(), everything);

    let client_text = client.display().to_string();
    let tracked = machine.json(&["track", &client_text, "--json"]).unwrap();
    let real = std::fs::canonicalize(&client).unwrap();
    assert_eq!(tracked["folders"], json!([real.display().to_string()]));
    assert_eq!(sessions(&machine).unwrap(), ["below-a", "in-a"]);
    let config = machine.tc(&["config", "folders"]).unwrap();
    assert_eq!(config.trim(), real.display().to_string());

    // The mod knows its own session, wherever it runs.
    machine
        .tc_with(
            &["record", "--json"],
            &[],
            &json!({"agent_harness": "claude-code", "session_id": "in-b", "summary": "Fixed it"})
                .to_string(),
        )
        .unwrap();
    assert_eq!(sessions(&machine).unwrap(), ["below-a", "in-a", "in-b"]);

    let untracked = machine.json(&["untrack", &client_text, "--json"]).unwrap();
    assert_eq!(untracked["folders"], json!([]));
    assert_eq!(sessions(&machine).unwrap(), everything);
    assert_eq!(machine.tc(&["config", "folders"]).unwrap().trim(), "all");

    let missing = machine.work("no-such-client").display().to_string();
    let output = machine.run(&["track", &missing], &[], "").unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("does not exist")
    );
    let output = machine.run(&["untrack", &client_text], &[], "").unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn track_with_no_folder_tracks_the_current_one() {
    let machine = Machine::ready().unwrap();
    let storefront = machine.work("storefront");
    let output = machine
        .command(&["track"])
        .current_dir(&storefront)
        .output()
        .unwrap();
    assert!(output.status.success());
    let real = std::fs::canonicalize(&storefront).unwrap();
    let said = String::from_utf8(output.stdout).unwrap();
    assert_eq!(said.trim(), format!("tracking {}", real.display()));
    let review = machine.json(&["review", "--json"]).unwrap();
    assert_eq!(
        by_harness(&review).keys().collect::<Vec<_>>(),
        ["claude-code"]
    );
}

#[test]
fn list_shows_where_sessions_ran_and_which_folders_are_tracked() {
    let machine = Machine::ready().unwrap();
    let list = machine.json(&["list", "--json"]).unwrap();
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    // The most recent first: the Codex session in ops ran later.
    let ops = &rows[0];
    let fields: Vec<&str> = ops
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        ["folder", "harnesses", "last_at", "sessions", "tracked"]
    );
    assert_eq!(ops["folder"], machine.work("ops").display().to_string());
    assert_eq!(ops["harnesses"], json!(["codex"]));
    assert_eq!(ops["sessions"], 1);
    assert_eq!(ops["last_at"], "2026-10-11T10:05:00Z");
    assert_eq!(ops["tracked"], true);
    assert_eq!(rows[1]["harnesses"], json!(["claude-code"]));

    machine
        .tc(&["track", &machine.work("storefront").display().to_string()])
        .unwrap();
    let list = machine.json(&["list", "--json"]).unwrap();
    assert_eq!(list[0]["tracked"], false);
    assert_eq!(list[1]["tracked"], true);

    let table = machine.tc(&["list"]).unwrap();
    assert!(table.contains("Claude Code"), "{table}");
    assert!(table.contains("2026-10-11"), "{table}");
}
