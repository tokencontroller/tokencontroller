# How `tc` reads sessions, places entries, signs in and ships

This fills in ADR 0010 where the CLI needed details.

## Reading session files

- **Readers of our own, following ccusage's rules.** ccusage's adapters are not published as crates on crates.io, and `deny.toml` refuses git sources. They also drop what placing needs: the branch, the message IDs and the prompts. So `tc` has its own readers for Claude Code and Codex in `apps/cli/src/sources/`, which follow ccusage's counting rules (named in each reader) and credit it in `apps/cli/THIRD_PARTY_NOTICES.md`.
- **Claude Code**: every `*.jsonl` under `CLAUDE_CONFIG_DIR` (comma-separated, each with `projects/`), otherwise `~/.config/claude/projects` and `~/.claude/projects`. A response is written as one line per content block with the same usage, so model calls count once by message ID; a subagent's replay of its parent counts once too, as the parent's.
- **Codex**: every rollout under `CODEX_HOME` (default `~/.codex`) in `sessions/` and `archived_sessions/`. Turns come from `token_count` events; a repeated running total counts nothing; cached input is part of input and is counted as cache read.
- Prompts and titles are read on the machine for ticket keys and never stored or sent; only the keys are.

## Placing

- The order is the one on the Rules page. `tc` places with `apps/cli/src/placing.rs` while it moves to the rules engine in `crates/core` (ADR 0015), so the machine and the service place alike; the command line does not change with it.
- Segments: a session splits exactly by message where the branch, the working directory or the subagent changes. Hand splits and equal splits divide by share with the largest-remainder method (ADR 0004).
- List value on the machine is an estimate in USD, the price list's currency: the agent's reported cost where it has one, otherwise the tokens at a price snapshot taken from LiteLLM's list and built into the binary (`apps/cli/prices.json`). The service computes the list value that counts with its own price tables.

## Commands for other programs

- Every command takes `--json` and then prints exactly one JSON value. Errors go to standard error. Exit code 1 is the caller's mistake (including a wrong command line), 2 an error from the service, 3 a local failure.
- `tc record` takes what the Claude Code mod knows that the files do not: the summary and the tags picked in the session, as one JSON object or one per line. Lines with a turn's usage are taken and not counted again, since the session files hold the same turns.
- `tc submit --tag NAME=VALUE --signal NAME=VALUE` is how the GitHub Action names its project, ticket and tags and passes the repository, branch and pull request of the job. `TC_URL`, `TC_TOKEN`, `TC_TICKET`, `TC_PROJECT`, `TC_TAGS` and `TC_SIGNALS` set the same for a whole run without touching the settings file. `TC_HOME` moves the settings file and the database.

## Signing in

`tc connect` uses a device sign-in like RFC 8628: the service gives a short code, the person confirms it in the browser while signed in, and `tc` receives a `tc_cli_…` token. That token belongs to one person on one machine, can be revoked under _Settings_, and may only submit that person's token sheet, read what placing needs and edit that person's rules. With a `tc_svc_…` service token, as in CI, `tc submit` reports automatic token entries instead of a token sheet.

## Shipping

- A tag `v1.0.0` runs `.github/workflows/release.yml`: it builds `tc` for macOS, Linux and Windows on Intel and ARM, publishes the archives and `SHA256SUMS` on the GitHub release, and writes the Homebrew formula to `Formula/tc.rb` in this repository, so `brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller` works without a separate tap repository. The same workflow writes the winget manifest and submits it when a `WINGET_TOKEN` secret is set.
- Each archive holds the binary, the MIT license, the third-party notices with the licenses of every Rust dependency, and the reference classifier script.

## Why

Readers we own can keep what placing needs and still count exactly as ccusage does, with nothing fetched at build time. One JSON contract and one exit code scheme let the mod and the Action wrap `tc` without parsing tables. Device sign-in keeps passwords and sign-in links out of the terminal.
