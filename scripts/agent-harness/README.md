# Agent harness

Every supported agent CLI runs a tiny task for real and reports to a stand-in for our ingest. The run fails when what arrived no longer reads as the docs say (ADR 0025). This is how a silent change in an agent's usage or telemetry format shows up before a customer's numbers go wrong.

## What one run does

For each agent and each task in the tasks file, `run.py`:

1. starts a capture server that plays the receiver at `/api/v1/ingest/otlp` and keeps every body, byte for byte;
2. gives the agent a fresh home, the telemetry settings from the docs page Collect > Telemetry push, and a temporary working folder, then runs the task through the agent's own CLI;
3. decodes the captured bodies with `tokencontroller otlp-decode`, the receiver's own code, and reads the session files with `tc export --csv`;
4. checks that telemetry, session files and the agent's own usage report describe the same model calls with the same token counts.

| Check | Passes when |
| --- | --- |
| `ran`, `answered` | The CLI exits 0 and its output holds the expected word. |
| `telemetry-arrived` | Bodies reached the ingest URL and every one decodes. |
| `telemetry-reads-as-model-calls` | At least one model call, read as this agent harness, with a model, a session id and output tokens. |
| `telemetry-matches-agent-report` | The counts the agent printed itself equal the telemetry totals, kind by kind. |
| `session-files-read` | `tc` reads the agent's session files (Claude Code and Codex). |
| `session-files-match-telemetry` | Every message in the files has a telemetry twin with the same counts. Telemetry may hold more: Claude Code's side calls are only there. |
| `doors-join-by-message-id` | A warning today: Claude Code telemetry names the request (`req_…`), the session file the message (`msg_…`), and Codex gives no id at all, so the join of ADR 0018 does not happen yet. Becomes a failure once fixed. |

## Running it

```sh
cargo build -p tokencontroller-server -p tokencontroller-cli
python3 scripts/agent-harness/run.py                      # every agent with a CLI and a key
python3 scripts/agent-harness/run.py --agents codex-cli   # one agent
python3 scripts/agent-harness/run.py --replay             # only the recorded fixtures, no key needed
```

Results: `target/agent-harness/results.jsonl`, one record per agent and task, and `summary.md`. In GitHub Actions the summary also lands on the run page. Exit code 1 when a check fails, 2 when the runner cannot work.

An agent is skipped when its CLI is not installed or none of its key variables is set: `ANTHROPIC_API_KEY` (or `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_BASE_URL`) for Claude Code, `OPENAI_API_KEY` for Codex, `GEMINI_API_KEY` for Gemini CLI, `COPILOT_GITHUB_TOKEN` or `GH_TOKEN` for Copilot CLI. `--require a,b` makes a skip a failure. `HARNESS_CLAUDE_MODEL` and `HARNESS_CODEX_MODEL` pick a cheaper model.

Claude Code and Codex are tested. Gemini CLI and Copilot CLI are wired from the docs and have not run here yet; their first green run makes them tested. Cursor has no CLI to run and keeps its hand-written fixture in `apps/server/tests/fixtures/otlp/`.

## Recorded fixtures

`--record` runs the agents with OTLP as JSON and keeps each body that holds a model call, scrubbed of names, hosts and ids, under `apps/server/tests/fixtures/otlp/recorded/`, named `<agent>-<version>-<signal>-<n>.json`. The receiver's test `recorded_agent_bodies_read_as_their_agent_harness` decodes every one on each pull request, so the parser never regresses against bytes a real agent sent. Record again after an agent release that changes its events, and keep the old version's files until the docs drop that version.

## Where it runs

- `.github/workflows/agent-harness.yml`, started by hand or by the weekly Azure clock job in the private repo. Agents whose key is not a `HARNESS_*` repository secret are skipped.
- The weekly agent format watch (private skill `watching-agent-formats`) runs it in a cloud session, where Claude Code and Codex have credentials, and reads the agents' changelogs.

## The same runner for the estimates benchmark

A task is `{"id", "prompt", "expect_output_contains"?, "files"?: {"path": "text"}, ...}`. The runner puts `files` into the working folder before the run and carries every other field, such as an estimate, through to the result record unchanged, next to the actual counts by kind, the duration and the agent's version. A benchmark is a tasks file with real tasks and `--out` somewhere kept; pricing the counts is the price table's job, not the runner's.
