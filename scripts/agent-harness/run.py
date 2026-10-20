#!/usr/bin/env python3
"""The agent harness check: every supported agent CLI runs a tiny task for
real and reports to a stand-in for our ingest, and the run fails when what
arrived no longer reads as the docs say it does.

For each agent and task the runner:

1. starts a capture server that plays the OpenTelemetry receiver at
   /api/v1/ingest/otlp and keeps every body it receives, byte for byte;
2. gives the agent a fresh home (its settings and session files land there),
   the telemetry settings from the docs page Collect > Telemetry push, and a
   temporary working folder, then runs the task through the agent's own CLI;
3. decodes the captured bodies with `tokencontroller otlp-decode`, the
   receiver's own code, and reads the session files with `tc review`;
4. checks that telemetry, session files and the agent's own usage report all
   describe the same model calls with the same token counts.

Results go to OUT/results.jsonl, one record per agent and task, with the
agent's version, duration, exit code, the counts from every source and each
check's outcome. OUT/summary.md is the same in prose. The estimates
benchmark uses the same runner with a different tasks file: a task may carry
`files` to put in the working folder and any `estimate` fields, which the
record carries through unchanged next to the actual counts.

Usage:
  run.py [--tasks FILE] [--agents a,b] [--out DIR] [--tc PATH] [--decoder PATH]
         [--require a,b] [--record DIR] [--timeout SECONDS]
  run.py --replay [--decoder PATH] [--out DIR]

An agent whose CLI or credentials are missing is skipped and named in the
summary; --require makes a skip of that agent a failure. --record also saves
each captured body, scrubbed of names and hosts, as a JSON fixture for the
receiver's tests (apps/server/tests/fixtures/otlp/recorded/). --replay only
decodes those fixtures, which needs no agent and no key. Exit code 1 when a
check fails, 2 when the runner itself cannot work.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import http.server
import io
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
RECORDED = REPO / "apps" / "server" / "tests" / "fixtures" / "otlp" / "recorded"
INGEST_PATH = "/api/v1/ingest/otlp"
ORG_TOKEN = "tc_org_harness"
SETTLE_SECONDS = 6  # exporters flush on an interval; wait for the last batch


# --- Agents -----------------------------------------------------------------


@dataclass
class Agent:
    """One agent harness: how to find it, run it and read what it sends."""

    name: str  # the agent harness name the service stores
    binary: str
    home_env: str | None  # the variable that moves its settings and sessions
    credential_envs: tuple[str, ...]  # any one set means it can call a model
    reads_session_files: bool  # tc reads this agent's session files
    protocol_json: bool  # the agent can send OTLP as JSON (for --record)
    tested: bool = True  # False: wired from the docs, never run by us yet
    version_args: tuple[str, ...] = ("--version",)

    def available(self) -> str | None:
        """Why the agent cannot run here, or None when it can."""
        if shutil.which(self.binary) is None:
            return f"`{self.binary}` is not installed"
        if not any(os.environ.get(name) for name in self.credential_envs):
            return "no credentials (" + " or ".join(self.credential_envs) + ")"
        return None

    def version(self) -> str:
        try:
            out = subprocess.run(
                [self.binary, *self.version_args],
                capture_output=True,
                text=True,
                timeout=60,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            return "unknown"
        text = (out.stdout + out.stderr).strip()
        match = re.search(r"\d+\.\d+\.\d+", text)
        return match.group(0) if match else (text.splitlines() or ["unknown"])[0]


AGENTS: dict[str, Agent] = {
    "claude-code": Agent(
        name="claude-code",
        binary="claude",
        home_env="CLAUDE_CONFIG_DIR",
        credential_envs=("ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_BASE_URL"),
        reads_session_files=True,
        protocol_json=True,
    ),
    "codex-cli": Agent(
        name="codex-cli",
        binary="codex",
        home_env="CODEX_HOME",
        credential_envs=("OPENAI_API_KEY", "CODEX_API_KEY"),
        reads_session_files=True,
        protocol_json=True,
    ),
    "gemini-cli": Agent(
        name="gemini-cli",
        binary="gemini",
        home_env=None,  # settings live in ~/.gemini; telemetry comes from variables
        credential_envs=("GEMINI_API_KEY", "GOOGLE_API_KEY"),
        reads_session_files=False,
        protocol_json=False,
        tested=False,
    ),
    "copilot-cli": Agent(
        name="copilot-cli",
        binary="copilot",
        home_env=None,
        credential_envs=("COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"),
        reads_session_files=False,
        protocol_json=False,
        tested=False,
    ),
}

# The session-file harness names `tc review` prints, per agent.
TC_HARNESS_NAMES = {"claude-code": "claude-code", "codex-cli": "codex"}


def command_and_env(
    agent: Agent, prompt: str, endpoint: str, home: Path, record: bool
) -> tuple[list[str], dict[str, str]]:
    """The agent's command line for one prompt, and the environment that
    switches its telemetry on as the docs page Collect > Telemetry push says.
    With `record` the agent sends OTLP as JSON where it can, so the capture
    can be scrubbed and kept as a fixture."""
    env = dict(os.environ)
    if agent.home_env:
        env[agent.home_env] = str(home)
    if agent.name == "claude-code":
        env.update(
            {
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_METRICS_EXPORTER": "otlp",
                "OTEL_LOGS_EXPORTER": "otlp",
                "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json" if record else "http/protobuf",
                "OTEL_EXPORTER_OTLP_ENDPOINT": endpoint,
                "OTEL_EXPORTER_OTLP_HEADERS": f"Authorization=Bearer {ORG_TOKEN}",
                "OTEL_METRICS_INCLUDE_REPOSITORY": "true",
                # Flush fast so the run ends quickly; the shape is the same.
                "OTEL_LOGS_EXPORT_INTERVAL": "1000",
                "OTEL_METRIC_EXPORT_INTERVAL": "1000",
            }
        )
        command = ["claude", "-p", prompt, "--output-format", "json"]
        model = os.environ.get("HARNESS_CLAUDE_MODEL")
        if model:
            command += ["--model", model]
        return command, env
    if agent.name == "codex-cli":
        protocol = "json" if record else "binary"
        (home / "config.toml").write_text(
            "[otel]\n"
            "log_user_prompt = false\n"
            "exporter = { otlp-http = { "
            f'endpoint = "{endpoint}/v1/logs", protocol = "{protocol}", '
            f'headers = {{ "Authorization" = "Bearer {ORG_TOKEN}" }} }} }}\n'
        )
        command = ["codex", "exec", "--skip-git-repo-check", "--json", prompt]
        model = os.environ.get("HARNESS_CODEX_MODEL")
        if model:
            command[2:2] = ["--model", model]
        return command, env
    if agent.name == "gemini-cli":
        env.update(
            {
                "GEMINI_TELEMETRY_ENABLED": "true",
                "GEMINI_TELEMETRY_OTLP_ENDPOINT": endpoint,
                "GEMINI_TELEMETRY_OTLP_PROTOCOL": "http",
                "GEMINI_TELEMETRY_OTLP_HEADERS": f"Authorization=Bearer {ORG_TOKEN}",
                "GEMINI_TELEMETRY_LOG_PROMPTS": "false",
            }
        )
        return ["gemini", "-p", prompt, "--output-format", "json", "--approval-mode", "plan"], env
    if agent.name == "copilot-cli":
        env.update(
            {
                "OTEL_EXPORTER_OTLP_ENDPOINT": endpoint,
                "OTEL_EXPORTER_OTLP_HEADERS": f"Authorization=Bearer {ORG_TOKEN}",
            }
        )
        return ["copilot", "-p", prompt, "--allow-all-tools"], env
    raise ValueError(agent.name)


def agent_reported_usage(agent: Agent, stdout: str) -> dict[str, int] | None:
    """The token counts the agent itself printed, in our four kinds, or
    None when the agent prints none we know how to read."""
    try:
        if agent.name == "claude-code":
            # `modelUsage` holds every call of the run, side calls included;
            # `usage` only the main turn.
            totals = {"input": 0, "output": 0, "cache_write": 0, "cache_read": 0}
            models = json.loads(stdout)["modelUsage"]
            for usage in models.values():
                totals["input"] += int(usage.get("inputTokens", 0))
                totals["output"] += int(usage.get("outputTokens", 0))
                totals["cache_write"] += int(usage.get("cacheCreationInputTokens", 0))
                totals["cache_read"] += int(usage.get("cacheReadInputTokens", 0))
            return totals if models else None
        if agent.name == "codex-cli":
            totals = {"input": 0, "output": 0, "cache_write": 0, "cache_read": 0}
            seen = False
            for line in stdout.splitlines():
                if '"turn.completed"' not in line:
                    continue
                usage = json.loads(line).get("usage") or {}
                seen = True
                cached = int(usage.get("cached_input_tokens", 0))
                written = int(usage.get("cache_write_input_tokens", 0))
                totals["input"] += int(usage.get("input_tokens", 0)) - cached - written
                totals["output"] += int(usage.get("output_tokens", 0))
                totals["cache_write"] += written
                totals["cache_read"] += cached
            return totals if seen else None
        if agent.name == "gemini-cli":
            models = json.loads(stdout).get("stats", {}).get("models", {})
            totals = {"input": 0, "output": 0, "cache_write": 0, "cache_read": 0}
            for stats in models.values():
                tokens = stats.get("tokens", {})
                cached = int(tokens.get("cached", 0))
                totals["input"] += int(tokens.get("prompt", 0)) - cached
                totals["output"] += int(tokens.get("candidates", 0))
                totals["cache_read"] += cached
            return totals if models else None
    except (ValueError, KeyError, TypeError):
        return None
    return None


# --- The capture server -----------------------------------------------------


@dataclass
class Capture:
    path: str
    headers: dict[str, str]
    body: bytes
    received_at: float


class CaptureServer:
    """Plays the receiver: answers every POST under the ingest path with an
    empty success and keeps the body. Anything else is a 404, as the service
    would answer, and is kept too so a wrong path shows in the results."""

    def __init__(self) -> None:
        self.captures: list[Capture] = []
        self.lock = threading.Lock()
        captures, lock = self.captures, self.lock

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 (http.server's name)
                length = int(self.headers.get("content-length") or 0)
                body = self.rfile.read(length)
                with lock:
                    captures.append(
                        Capture(self.path, {k.lower(): v for k, v in self.headers.items()}, body, time.time())
                    )
                ok = self.path.startswith(INGEST_PATH + "/v1/")
                self.send_response(200 if ok else 404)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(b"{}")

            def log_message(self, *_args: object) -> None:
                pass

        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            self.port = probe.getsockname()[1]
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def endpoint(self) -> str:
        return f"http://127.0.0.1:{self.port}{INGEST_PATH}"

    def __enter__(self) -> CaptureServer:
        self.thread.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.server.shutdown()
        self.server.server_close()


# --- Decoding what arrived ---------------------------------------------------


def find_tool(explicit: str | None, name: str, package: str) -> list[str]:
    """The command for one of our binaries: the given path, a built one, or
    `cargo run`."""
    if explicit:
        return [explicit]
    for profile in ("release", "debug"):
        candidate = REPO / "target" / profile / name
        if candidate.exists():
            return [str(candidate)]
    return ["cargo", "run", "--quiet", "-p", package, "--"]


def signal_of(path: str) -> str | None:
    tail = path.rsplit("/v1/", 1)
    return tail[1].split("?")[0] if len(tail) == 2 and tail[1] in ("logs", "traces", "metrics") else None


def decode(decoder: list[str], capture: Capture) -> dict:
    """What the receiver makes of one captured body, through its own code."""
    signal = signal_of(capture.path)
    if signal is None:
        return {"error": f"unexpected path {capture.path}", "messages": []}
    with tempfile.NamedTemporaryFile(delete=False) as handle:
        handle.write(capture.body)
        name = handle.name
    try:
        command = [*decoder, "otlp-decode", signal]
        if "content-type" in capture.headers:
            command += ["--content-type", capture.headers["content-type"]]
        if "content-encoding" in capture.headers:
            command += ["--content-encoding", capture.headers["content-encoding"]]
        out = subprocess.run([*command, name], capture_output=True, text=True, check=False, cwd=REPO)
    finally:
        os.unlink(name)
    if out.returncode != 0:
        return {"error": out.stderr.strip() or f"exit {out.returncode}", "messages": [], "signal": signal}
    try:
        return json.loads(out.stdout)
    except ValueError as error:
        return {"error": f"decoder printed no JSON: {error}", "messages": [], "signal": signal}


def total(messages: list[dict]) -> dict[str, int]:
    totals = {"input": 0, "output": 0, "cache_write": 0, "cache_read": 0}
    for message in messages:
        for kind in totals:
            totals[kind] += int(message["tokens"][kind])
    return totals


def session_files(tc: list[str], agent: Agent, home: Path, tc_home: Path) -> dict:
    """What `tc` reads from the agent's session files in `home`: one row per
    message from `tc export --csv`, with its ids and counts."""
    env = dict(os.environ)
    env["TC_HOME"] = str(tc_home)
    if agent.home_env:
        env[agent.home_env] = str(home)
    out = subprocess.run(
        [*tc, "export", "--csv"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
        cwd=REPO,
        stdin=subprocess.DEVNULL,
    )
    if out.returncode != 0:
        return {"error": out.stderr.strip() or f"exit {out.returncode}", "messages": []}
    rows = list(csv.DictReader(io.StringIO(out.stdout)))
    wanted = TC_HARNESS_NAMES.get(agent.name)
    messages = []
    for row in rows:
        if row.get("agent_harness") != wanted:
            continue
        try:
            messages.append(
                {
                    "session_id": row["session_id"],
                    "message_id": row["message_id"],
                    "model": row["model"],
                    "tokens": {kind: int(row[f"tokens.{kind}"] or 0) for kind in ("input", "output", "cache_write", "cache_read")},
                }
            )
        except (KeyError, ValueError) as error:
            return {"error": f"tc export has no {error} column", "messages": []}
    return {"messages": messages}


# --- Scrubbed fixtures -------------------------------------------------------

SCRUB_KEYS = {
    "user.email": "person@example.com",
    "user.account_uuid": "00000000-0000-0000-0000-000000000000",
    "user.id": "person",
    "user.account_id": "00000000-0000-0000-0000-000000000000",
    "ccr.session.id": "00000000-0000-0000-0000-000000000000",
    "session.id": "11111111-1111-4111-8111-111111111111",
    "enduser.id": "person",
    "enduser.pseudo.id": "person",
    "host.name": "workstation",
    "host.arch": "arm64",
    "os.type": "linux",
    "os.version": "0",
    "terminal.type": "terminal",
    "organization.id": "00000000-0000-0000-0000-000000000000",
}


def scrub(value: object) -> object:
    """Replaces names, hosts and anything that looks like an address in an
    OTLP JSON body, so a fixture names nobody."""
    if isinstance(value, dict):
        if "key" in value and value.get("key") in SCRUB_KEYS and isinstance(value.get("value"), dict):
            return {"key": value["key"], "value": {"stringValue": SCRUB_KEYS[value["key"]]}}
        return {k: scrub(v) for k, v in value.items()}
    if isinstance(value, list):
        return [scrub(item) for item in value]
    if isinstance(value, str):
        return re.sub(r"[\w.+-]+@[\w-]+\.[\w.-]+", "person@example.com", value)
    return value


def record_fixture(
    directory: Path, agent: Agent, version: str, capture: Capture, decoded: dict, index: int
) -> Path | None:
    """Keeps a JSON body that holds a model call, or the first metrics body,
    as a fixture. Bodies with only startup or tool events teach nothing."""
    if "json" not in capture.headers.get("content-type", ""):
        return None
    signal = signal_of(capture.path)
    if signal is None or decoded.get("error"):
        return None
    if signal == "metrics":
        if any(existing.stem.startswith(f"{agent.name}-{version}-metrics") for existing in directory.glob("*.json")):
            return None
    elif not decoded.get("messages"):
        return None
    try:
        body = json.loads(capture.body)
    except ValueError:
        return None
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / f"{agent.name}-{version}-{signal}-{index}.json"
    path.write_text(json.dumps(scrub(body), indent=2, sort_keys=True) + "\n")
    return path


# --- Running one task on one agent ------------------------------------------


@dataclass
class Check:
    name: str
    outcome: str  # pass, fail, warn (a known gap, not a failure) or skip
    note: str = ""


@dataclass
class Result:
    task: dict
    agent: str
    version: str
    started_at: str
    duration_s: float | None = None
    exit_code: int | None = None
    agent_reported: dict | None = None
    telemetry: dict = field(default_factory=dict)
    session_files: dict = field(default_factory=dict)
    checks: list[Check] = field(default_factory=list)
    output_tail: str = ""
    fixtures: list[str] = field(default_factory=list)

    def failed(self) -> bool:
        return any(check.outcome == "fail" for check in self.checks)

    def to_json(self) -> dict:
        record = dict(self.__dict__)
        record["checks"] = [check.__dict__ for check in self.checks]
        return record


def run_task(agent: Agent, task: dict, tools: dict, timeout: int, record_dir: Path | None) -> Result:
    result = Result(task=task, agent=agent.name, version=agent.version(), started_at=now())
    with tempfile.TemporaryDirectory(prefix=f"harness-{agent.name}-") as temp, CaptureServer() as capture:
        root = Path(temp)
        home, work, tc_home = root / "home", root / "work", root / "tc"
        for folder in (home, work, tc_home):
            folder.mkdir()
        for name, text in (task.get("files") or {}).items():
            target = work / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        subprocess.run(["git", "init", "--quiet", str(work)], check=False, capture_output=True)
        command, env = command_and_env(agent, task["prompt"], capture.endpoint, home, record_dir is not None)
        env["HOME"] = env.get("HOME", str(root))
        started = time.monotonic()
        try:
            out = subprocess.run(
                command,
                cwd=work,
                env=env,
                capture_output=True,
                text=True,
                timeout=timeout,
                check=False,
                stdin=subprocess.DEVNULL,
            )
            result.exit_code = out.returncode
            stdout, stderr = out.stdout, out.stderr
        except subprocess.TimeoutExpired as expired:
            result.exit_code = None
            stdout = (expired.stdout or b"").decode(errors="replace") if isinstance(expired.stdout, bytes) else (expired.stdout or "")
            stderr = f"timed out after {timeout} seconds"
        result.duration_s = round(time.monotonic() - started, 1)
        result.output_tail = (stdout + "\n" + stderr)[-2000:]
        time.sleep(SETTLE_SECONDS)

        result.checks.append(
            Check("ran", "pass" if result.exit_code == 0 else "fail", f"exit code {result.exit_code}")
        )
        expected = task.get("expect_output_contains")
        if expected:
            found = expected.lower() in stdout.lower()
            result.checks.append(Check("answered", "pass" if found else "fail", f"output contains {expected!r}"))

        result.agent_reported = agent_reported_usage(agent, stdout)

        # Telemetry: every captured body through the receiver's decoder.
        messages: list[dict] = []
        decoded_bodies = []
        errors = []
        for index, item in enumerate(capture.captures):
            decoded = decode(tools["decoder"], item)
            decoded_bodies.append(
                {
                    "path": item.path,
                    "content_type": item.headers.get("content-type"),
                    "content_encoding": item.headers.get("content-encoding"),
                    "bytes": len(item.body),
                    "messages": len(decoded.get("messages", [])),
                    "ignored": decoded.get("ignored"),
                    "dropped_with_prompt_text": decoded.get("dropped_with_prompt_text"),
                    "error": decoded.get("error"),
                }
            )
            if decoded.get("error"):
                errors.append(f"{item.path}: {decoded['error']}")
            messages.extend(decoded.get("messages", []))
            if record_dir is not None:
                saved = record_fixture(record_dir, agent, result.version, item, decoded, index)
                if saved:
                    result.fixtures.append(str(saved.relative_to(REPO)) if saved.is_relative_to(REPO) else str(saved))
        result.telemetry = {"requests": decoded_bodies, "messages": messages, "totals": total(messages)}

        if not capture.captures:
            result.checks.append(Check("telemetry-arrived", "fail", "the agent sent nothing to the ingest URL"))
        elif errors:
            result.checks.append(Check("telemetry-arrived", "fail", "; ".join(errors)[:500]))
        else:
            result.checks.append(Check("telemetry-arrived", "pass", f"{len(capture.captures)} requests"))

        if messages:
            wrong_harness = [m["harness"] for m in messages if m["harness"] != agent.name]
            no_model = [m for m in messages if not m["model"]]
            no_session = [m for m in messages if not m["session_id"]]
            problems = []
            if wrong_harness:
                problems.append(f"read as {sorted(set(wrong_harness))}")
            if no_model:
                problems.append(f"{len(no_model)} without a model")
            if no_session:
                problems.append(f"{len(no_session)} without a session id")
            if result.telemetry["totals"]["output"] == 0:
                problems.append("no output tokens")
            result.checks.append(
                Check(
                    "telemetry-reads-as-model-calls",
                    "fail" if problems else "pass",
                    "; ".join(problems) or f"{len(messages)} model calls, {result.telemetry['totals']}",
                )
            )
        else:
            result.checks.append(
                Check("telemetry-reads-as-model-calls", "fail", "no captured body held a model call the receiver reads")
            )

        if result.agent_reported is not None and messages:
            same = result.agent_reported == result.telemetry["totals"]
            result.checks.append(
                Check(
                    "telemetry-matches-agent-report",
                    "pass" if same else "fail",
                    f"agent {result.agent_reported}, telemetry {result.telemetry['totals']}",
                )
            )
        else:
            result.checks.append(Check("telemetry-matches-agent-report", "skip", "the agent printed no usage we read"))

        # Session files, through tc: every call in the files must be in
        # telemetry with the same counts. Telemetry may hold more: side calls
        # Claude Code makes that its session file leaves out.
        if agent.reads_session_files:
            read = session_files(tools["tc"], agent, home, tc_home)
            file_messages = read.get("messages", [])
            file_totals = total(file_messages) if file_messages else None
            result.session_files = {"messages": file_messages, "totals": file_totals, "error": read.get("error")}
            if read.get("error"):
                result.checks.append(Check("session-files-read", "fail", read["error"][:500]))
            elif not file_messages:
                result.checks.append(Check("session-files-read", "fail", "tc found no message of this agent"))
            else:
                result.checks.append(Check("session-files-read", "pass", f"{len(file_messages)} messages, {file_totals}"))
                if messages:
                    unmatched = [
                        m for m in file_messages
                        if not any(t["session_id"] == m["session_id"] and t["tokens"] == m["tokens"] for t in messages)
                    ]
                    extra = len(messages) - (len(file_messages) - len(unmatched))
                    result.checks.append(
                        Check(
                            "session-files-match-telemetry",
                            "fail" if unmatched else "pass",
                            f"{len(unmatched)} of {len(file_messages)} file messages have no telemetry twin"
                            if unmatched
                            else f"every file message has a telemetry twin; telemetry holds {extra} more (side calls)",
                        )
                    )
                    # The join the service makes (ADR 0018) is by message id.
                    # Claude Code's telemetry and session file share the API
                    # request id (req_...), so every file message must carry
                    # an id telemetry also sent, or both doors on count a
                    # call twice. Codex sends no id in telemetry, so there
                    # the join cannot happen and the check only warns.
                    ids = {t["message_id"] for t in messages}
                    joined = [m for m in file_messages if m["message_id"] in ids]
                    all_joined = len(joined) == len(file_messages)
                    result.checks.append(
                        Check(
                            "doors-join-by-message-id",
                            "pass" if all_joined else ("fail" if agent.name == "claude-code" else "warn"),
                            f"{len(joined)} of {len(file_messages)} file messages carry an id telemetry also sent",
                        )
                    )
        else:
            result.checks.append(Check("session-files-read", "skip", "tc does not read this agent's files"))
    return result


# --- Replay ------------------------------------------------------------------


def replay(decoder: list[str]) -> list[Result]:
    results = []
    for fixture in sorted(RECORDED.glob("*.json")):
        agent_name = fixture.name.rsplit("-", 3)[0]
        signal = fixture.stem.rsplit("-", 2)[1]
        version = fixture.name.rsplit("-", 3)[1]
        capture = Capture(f"{INGEST_PATH}/v1/{signal}", {"content-type": "application/json"}, fixture.read_bytes(), 0)
        decoded = decode(decoder, capture)
        result = Result(task={"id": "replay", "fixture": fixture.name}, agent=agent_name, version=version, started_at=now())
        messages = decoded.get("messages", [])
        result.telemetry = {"messages": messages, "totals": total(messages)}
        if decoded.get("error"):
            result.checks.append(Check("fixture-decodes", "fail", decoded["error"]))
        elif signal == "metrics":
            result.checks.append(Check("fixture-decodes", "pass", "metrics are acknowledged, never counted"))
        elif not messages:
            result.checks.append(Check("fixture-decodes", "fail", "no model call read from a recorded body that held one"))
        elif any(m["harness"] != agent_name for m in messages):
            result.checks.append(Check("fixture-decodes", "fail", f"read as {sorted({m['harness'] for m in messages})}"))
        else:
            result.checks.append(Check("fixture-decodes", "pass", f"{len(messages)} model calls, {result.telemetry['totals']}"))
        results.append(result)
    return results


# --- Reporting ---------------------------------------------------------------


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def summary(results: list[Result], skipped: dict[str, str]) -> str:
    lines = ["# Agent harness check", ""]
    for result in results:
        state = "FAIL" if result.failed() else "ok"
        task = result.task.get("id", "?")
        lines.append(f"## {result.agent} {result.version}, task `{task}`: {state}")
        lines.append("")
        if result.duration_s is not None:
            lines.append(f"Ran {result.duration_s} s, exit code {result.exit_code}.")
            lines.append("")
        for check in result.checks:
            mark = {"pass": "✓", "fail": "✗", "skip": "–", "warn": "!"}[check.outcome]
            lines.append(f"- {mark} {check.name}: {check.note}")
        if result.failed() and result.output_tail.strip():
            lines += ["", "```", result.output_tail.strip()[-1200:], "```"]
        lines.append("")
    if skipped:
        lines.append("## Skipped")
        lines.append("")
        for name, why in skipped.items():
            lines.append(f"- {name}: {why}")
        lines.append("")
    untested = [a.name for a in AGENTS.values() if not a.tested]
    if untested:
        lines.append(
            "Wired from the docs but never run by us yet: " + ", ".join(untested) + ". Their first green run makes them tested."
        )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tasks", default=str(HERE / "tasks" / "format-check.json"))
    parser.add_argument("--agents", default=",".join(AGENTS), help="comma-separated agent names")
    parser.add_argument("--require", default="", help="agents that may not be skipped")
    parser.add_argument("--out", default=str(REPO / "target" / "agent-harness"))
    parser.add_argument("--tc", help="path to the tc binary")
    parser.add_argument("--decoder", help="path to the tokencontroller binary")
    parser.add_argument("--record", nargs="?", const=str(RECORDED), help="save scrubbed JSON fixtures here")
    parser.add_argument("--replay", action="store_true", help="only decode the recorded fixtures")
    parser.add_argument("--timeout", type=int, default=600, help="seconds per agent run")
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    tools = {
        "decoder": find_tool(args.decoder, "tokencontroller", "tokencontroller-server"),
        "tc": find_tool(args.tc, "tc", "tokencontroller-cli"),
    }

    skipped: dict[str, str] = {}
    results: list[Result] = []
    if args.replay:
        results = replay(tools["decoder"])
        if not results:
            print("no recorded fixtures under", RECORDED, file=sys.stderr)
            return 2
    else:
        try:
            tasks = json.loads(Path(args.tasks).read_text())["tasks"]
        except (OSError, ValueError, KeyError) as error:
            print(f"cannot read tasks: {error}", file=sys.stderr)
            return 2
        names = [name.strip() for name in args.agents.split(",") if name.strip()]
        unknown = [name for name in names if name not in AGENTS]
        if unknown:
            print(f"unknown agents {unknown}; known: {sorted(AGENTS)}", file=sys.stderr)
            return 2
        required = {name.strip() for name in args.require.split(",") if name.strip()}
        record_dir = Path(args.record) if args.record else None
        for name in names:
            agent = AGENTS[name]
            why = agent.available()
            if why:
                skipped[name] = why
                continue
            for task in tasks:
                print(f"{name}: running task {task.get('id', '?')}", file=sys.stderr, flush=True)
                results.append(run_task(agent, task, tools, args.timeout, record_dir))
        for name in required & set(skipped):
            results.append(Result(task={"id": "required"}, agent=name, version="", started_at=now(), checks=[Check("available", "fail", skipped[name])]))

    with (out / "results.jsonl").open("w") as handle:
        for result in results:
            handle.write(json.dumps(result.to_json(), sort_keys=True) + "\n")
    text = summary(results, skipped)
    (out / "summary.md").write_text(text)
    print(text)
    step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if step_summary:
        with open(step_summary, "a") as handle:
            handle.write(text + "\n")
    return 1 if any(result.failed() for result in results) else 0


if __name__ == "__main__":
    sys.exit(main())
