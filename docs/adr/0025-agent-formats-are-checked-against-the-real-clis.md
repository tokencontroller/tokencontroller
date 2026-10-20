# Agent formats are checked against the real CLIs

Token Controller reads what agents write: Claude Code and Codex session files through `tc` (ADR 0017), and the telemetry of Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor through the receiver (ADR 0021). The agents change these formats without notice. A renamed field counts as zero, a new token kind lands in the wrong column, and nobody sees it until a customer's bill does not reconcile.

So the repository carries a harness, `scripts/agent-harness/`, that runs each agent's real CLI on a tiny task against a stand-in for the ingest URL and decodes what arrives with the receiver's own code, through `tokencontroller otlp-decode`. A run fails when telemetry, the session files `tc` reads and the agent's own usage report no longer agree on the model calls and their token counts. Bodies real agents sent are kept, scrubbed, as fixtures under `apps/server/tests/fixtures/otlp/recorded/`, and a unit test decodes every one on each pull request. The workflow `agent-harness.yml` runs the live check when the private clock starts it; the weekly format watch runs it in a cloud session and reads the agents' changelogs.

The same runner takes any tasks file, so the estimates benchmark runs real agents on real tasks and records the actual counts next to the estimate.

## Why

Hand-written fixtures test what we believed the format to be. Only the real CLI shows what it is this week. Decoding with the receiver's own code, not a copy, means the harness and the service cannot drift apart. Keeping the recorded bodies gives every pull request the same guard without a key.
