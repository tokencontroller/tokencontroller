---
title: "CLI"
description: "Every tc command, flag, setting and environment variable."
---

Every `tc` command. Each takes `--help`, and `tc` works with no service connected.

## Commands {#commands}

| Command | What it does |
| --- | --- |
| `tc connect URL` | Connect to an organization with an invite link, such as `https://app.tokencontroller.com/i/kestrel-4f2a`, or to a self-hosted instance by its address. |
| `tc status` | Organization, person, agents found, rules, today's cost and share on tickets, the token sheet's cadence period and whether it is late, and an organization manager's request to submit before the close. |
| `tc review` | List draft token entries and their tags. `--all` adds the submitted ones. |
| `tc tag N NAME=VALUE…` | Set tags on token entry N (its number from `tc review`), such as `ticket=KD-214` or `purpose=learning`. An empty value, such as `purpose=`, removes a tag. |
| `tc split N TICKET=SHARE…` | Split token entry N between two or more tickets, such as `KD-231=60 KD-240=40`. Shares are weights and need not add up to 100. |
| `tc submit` | Submit your token sheet. With no drafts it tells the service there was nothing to report, so the sheet is not late. For `--tag` and `--signal`, see [In CI and scripts](#scripts). |
| `tc report --by TAG\|day` | Cost in a table, by any tag name or by day. `--filter NAME=VALUE` limits it and can be repeated; `--since 2026-10-01` sets the first day (default: the first of this month). |
| `tc export --csv` | All token entries as CSV, the only format. |
| `tc rules` | List, add, move and remove personal rules. See [Rules](#rules). |
| `tc config [KEY [VALUE]]` | Show or change a setting. Without a key it lists every setting; an empty value removes one. See [Settings](#settings). |
| `tc record` | Add what the Claude Code mod or another tool knows about a session, as JSON on standard input. The mod runs it for you. |
| `tc track [DIR]` | Count only sessions in tracked folders and the folders below them; this adds DIR, default the current folder. See [Folders](#folders). |
| `tc untrack DIR` | Stop tracking a folder. Once none is left, every folder counts again. |
| `tc list` | The folders sessions ran in: agents, number of sessions, last activity and whether they count. |

## Folders {#folders}

```sh
tc list                          # where your sessions ran
tc track ~/work/storefront       # count only this folder and the folders below it
tc untrack ~/work/storefront
```

With no folder tracked, `tc` counts every Claude Code and Codex session on the machine. Once one is, a session counts only when it ran in a tracked folder or below it: `~/work/shop` covers `~/work/shop/api` but not `~/work/shop-old`. A session the [Claude Code mod](/collect/claude-code) reports counts wherever it ran. The folder is the working directory the session file names.

Drafts of sessions that no longer count are removed on the next run, along with anything set on them by hand. Submitted token entries stay as they are.

The first time a command reads sessions at a terminal while nothing is tracked, `tc` asks `Track sessions in <current folder>? [Y/n]`. Yes tracks that folder; no counts every folder and is not asked again. Scripts, CI, the GitHub Action and `--json` are never asked, and count every folder until you track one.

`tc track` and `tc untrack` print the tracked folders; with `--json`, `{"folders": [...]}`, empty when every folder counts. `tc track` exits with 1 when the folder does not exist, `tc untrack` when it was not tracked. `tc list --json` prints an array, most recent first, of `{"folder", "harnesses", "sessions", "last_at", "tracked"}`: `harnesses` holds `claude-code` and `codex`, `last_at` is a UTC time, `folder` is `null` for sessions whose files name none, and `tracked` is `true` when sessions there count.

## Rules {#rules}

```sh
tc rules                                                  # organization rules, then yours (also: tc rules list)
tc rules add branch starts-with review/ purpose=code-review
tc rules move 2 1                                         # earlier rules win
tc rules remove 1
```

A rule is a signal, a test, a value and the tags it sets. The signals are those on the [Rules](/attribute/rules#signals) page: `branch`, `directory`, `repository`, `pull-request` and `subagent`, plus `ticket`, the ticket placed so far, for rules that set other tags. The tests are `is`, `starts-with`, `contains`, `matches` (a regular expression) and `is-set`, which takes no value. Without a test, a value containing any of `^ $ * + ? ( ) [ ] { } | \` is a regular expression and any other value must match exactly.

Organization rules run before yours. Once connected, your rules live in your organization, so the web app and the mod show the same list.

## Settings {#settings}

Settings are kept in `config.toml` in the `tc` folder (see `TC_HOME` below).

| Setting | What it does |
| --- | --- |
| `classifier` | A program that picks the ticket when rules cannot. Empty switches it off. See [Classifier](/attribute/classifier). |
| `ticket-key-pattern` | A ticket key pattern, such as `^(KD\|OPS)-\d+`, for use with no service connected. Once connected, each project's pattern applies. |
| `since` | A day, such as `2026-09-01`. Sessions that started earlier get no token entry. Without it, the first run reaches back 31 days. |
| `folders` | The tracked folders, set with `tc track` and `tc untrack`. `all` counts every folder without asking; an empty value removes the choice, so `tc` asks again. See [Folders](#folders). |
| `service` | The service `tc` is connected to. Set by `tc connect`. |

## In CI and scripts {#scripts}

With `--json` every command prints one JSON value and nothing else. Messages go to standard error.

| Exit code | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | A mistake in the command, such as a bad argument or an unknown entry number, or a command that needs a connection while none is set up. |
| 2 | The service answered with an error or could not be reached. |
| 3 | A problem on this machine, such as a file that cannot be read or written. |

`tc submit --tag NAME=VALUE` sets a tag on every token entry it sends; it wins over every rule. `tc submit --signal NAME=VALUE` passes what the session files lack: `repository`, `branch`, `pull_request` or `pull_request_title`. Both can be repeated. The [GitHub Action](/collect/github-action) passes the same through the variables below.

| Variable | What it does |
| --- | --- |
| `TC_URL`, `TC_TOKEN` | The service and token to use instead of the ones `tc connect` saved. With a service token, `tc submit` reports automatic token entries. |
| `TC_TICKET`, `TC_PROJECT`, `TC_TAGS` | A ticket, a project, or tags such as `team=web,purpose=code-review` for every token entry in this run. |
| `TC_SIGNALS` | Signals for every token entry in this run, as JSON, such as `{"repository":"kestrel/storefront","pull_request":12}`. The names are those of `--signal`. |
| `TC_HOME` | The folder for `config.toml` and the local database `tc.sqlite`. Default: `tokencontroller` in the system's folder for application data (`$XDG_DATA_HOME` or `~/.local/share` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows). |
| `CLAUDE_CONFIG_DIR`, `CODEX_HOME` | Where Claude Code and Codex keep their session files, as the agents themselves read them. |
