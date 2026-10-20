# How `tc` reads sessions, splits them, signs in and ships

This fills in ADR 0010 where the CLI needed details.

## Reading session files

- **Readers of our own, following ccusage's rules.** ccusage's adapters are not published as crates on crates.io, and `deny.toml` refuses git sources. They also drop what splitting needs: the branch, the message IDs and the prompts. So `tc` has its own readers for Claude Code and Codex in `apps/cli/src/sources/`, which follow ccusage's counting rules (named in each reader) and credit it in `apps/cli/THIRD_PARTY_NOTICES.md`.
- **Claude Code**: every `*.jsonl` under `CLAUDE_CONFIG_DIR` (comma-separated, each with `projects/`), otherwise `~/.config/claude/projects` and `~/.claude/projects`. A response is written as one line per content block with the same usage, so model calls count once by message ID, and each takes its API request ID as its id, the id telemetry sends (ADR 0018); a subagent's replay of its parent counts once too, as the parent's.
- **Codex**: every rollout under `CODEX_HOME` (default `~/.codex`) in `sessions/` and `archived_sessions/`. Turns come from `token_count` events; a repeated running total counts nothing; cached input is part of input and is counted as cache read.
- Prompts and titles are read on the machine for feature keys and never stored or sent; only the keys are.

## Splitting

- The core splits: `tc` hands each session with its signals to the rules engine in `crates/core`, so the machine and the service split alike. Rules travel in the core's own shape. Connected, the organization's rules, the tag values the person may see, their attributes and rules, and the service's price table come from `GET /api/v1/me/context`; with no service connected, the default rules and the person's own apply, and a pattern from `tc config feature-key-pattern` finds feature keys.
- A session splits exactly by message where the branch, the working directory or the subagent changes. Hand splits and equal splits divide by fraction with the largest-remainder method (ADR 0004).
- `tc` ships no prices. Connected, it prices with the service's table from `GET /api/v1/me/context`; offline, with the person's own CSV (`provider,model,kind,price_per_million,valid_from`) named by `tc config price-table`. Without a row a message is an unknown model, and a message without counts is a count error; neither gets a cost.
- The local store is an event log in `tc.sqlite`, folded with `crates/core` as the service folds its own, so reports on the machine and in the service agree. The agent's reported cost is sent as a fact and never shown as the cost.

## Commands for other programs

- Every command takes `--json` and then prints exactly one JSON value. Errors go to standard error. Exit code 1 is the caller's mistake (including a wrong command line), 2 an error from the service, 3 a local failure.
- `tc record` takes what the Claude Code mod knows that the files do not: the summary and the tags picked in the session, as one JSON object or one per line. Lines with a turn's usage are taken and not counted again, since the session files hold the same turns.
- `tc submit --tag NAME=VALUE --signal NAME=VALUE` is how the GitHub Action names its project, feature and tags and passes the repository, branch and pull request of the job. `TC_URL`, `TC_TOKEN`, `TC_FEATURE`, `TC_PROJECT`, `TC_TAGS` and `TC_SIGNALS` set the same for a whole run without touching the settings file. `TC_HOME` moves the settings file and the database.

## Signing in

`tc connect` signs in with a magic link, as the web app does (`POST /api/v1/sign-in` with `local_tool`). With an invite link it takes the address from the invite; for an instance address it asks for it. The person presses Sign in in the mail, and `tc` collects a `tc_cli_…` token by polling and keeps it in its settings file. Opening the link alone uses nothing, so mail scanners cannot sign a machine in. That token belongs to one person on one machine, can be revoked under _Settings_, reads what its person may view, and may change only that person's own splits, submits and rules: settings, members and API tokens need the web app. With a `tc_svc_…` service token, as in CI, the messages carry the token's creator as their person.

## Shipping

- A tag `v1.0.0` runs `.github/workflows/release.yml`: it builds `tc` for macOS, Linux and Windows on Intel and ARM, publishes the archives and `SHA256SUMS` on the GitHub release, and writes the Homebrew formula to `Formula/tc.rb` in this repository, so `brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller` works without a separate tap repository. The same workflow writes the winget manifest and submits it when a `WINGET_TOKEN` secret is set.
- Each archive holds the binary, the MIT license and the third-party notices with the licenses of every Rust dependency.

## Why

Readers we own can keep what splitting needs and still count exactly as ccusage does, with nothing fetched at build time. One JSON contract and one exit code scheme let the mod and the Action wrap `tc` without parsing tables. Signing in with the web app's magic link keeps passwords out of the terminal and needs no second sign-in in the service.
