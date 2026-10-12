---
title: "CLI"
description: "Every tc command."
---

Every `tc` command.

## Commands {#commands}

| Command | What it does |
| --- | --- |
| `tc connect URL` | Connect to an organization with an invite link, or to a self-hosted instance. |
| `tc status` | Organization, person, agents found, rules, and today's cost and share on tickets. |
| `tc review` | List draft token entries and their tags. `--all` adds the submitted ones. |
| `tc tag N NAME=VALUE…` | Set tags on token entry N, such as `ticket=KD-214` or `purpose=learning`. An empty value removes a tag. |
| `tc split N TICKET=SHARE…` | Split token entry N between tickets, such as `KD-231=60 KD-240=40`. |
| `tc submit` | Submit your token sheet. With no drafts it tells the service there was nothing to report, so the sheet is not late. |
| `tc report --by TAG\|day` | Cost in a table, by any tag name or by day. `--filter team=web` limits it, `--since 2026-10-01` sets the first day. |
| `tc export --csv` | All token entries as CSV. |
| `tc rules` | List, add and order personal rules. |
| `tc config KEY VALUE` | Settings, such as `classifier`. |
| `tc record` | Add what the Claude Code mod knows about a session, as JSON on standard input. The mod runs it for you. |

Every command takes `--help`.

## Rules {#rules}

```sh
tc rules                                                  # organization rules, then yours
tc rules add branch starts-with review/ purpose=code-review
tc rules move 2 1                                         # earlier rules win
tc rules remove 1
```

A rule is a signal, a test, a value and the tags it sets. The signals are those on the [Rules](/attribute/rules#signals) page: `branch`, `directory`, `repository`, `pull-request` and `subagent`, plus `ticket`, the ticket placed so far, for rules that set other tags. The tests are `is`, `starts-with`, `contains`, `matches`, a regular expression, and `is-set`, which takes no value. Organization rules run before yours. Connected, your rules live in your organization, so the web app and the mod show the same list.

## Settings {#settings}

| Setting | What it does |
| --- | --- |
| `classifier` | A program that picks the ticket when rules cannot. See [Classifier](/attribute/classifier). |
| `ticket-key-pattern` | A ticket key pattern, such as `^(KD\|OPS)-\d+`, for use with no service connected. Connected, each project's pattern applies. |
| `since` | A day, such as `2026-09-01`. Sessions that started earlier get no token entry. Without it, the first run reaches back 31 days. |

## In CI and scripts {#scripts}

With `--json` every command prints one JSON value and nothing else. Messages go to standard error. The exit code is 0 on success, 1 for a mistake in the command or a missing connection, 2 when the service answered with an error, and 3 for a problem on the machine.

`tc submit --tag NAME=VALUE` names a tag for every token entry it sends, and `--signal NAME=VALUE` passes what the session files lack: `repository`, `branch`, `pull_request` or `pull_request_title`. The [GitHub Action](/collect/github-action) passes the same through the variables below.

| Variable | What it does |
| --- | --- |
| `TC_URL`, `TC_TOKEN` | The service and token to use instead of the ones `tc connect` saved. With a service token, `tc submit` reports automatic token entries. |
| `TC_TICKET`, `TC_PROJECT`, `TC_TAGS` | A ticket, a project, or tags such as `team=web,purpose=code-review` for every token entry in this run. |
| `TC_SIGNALS` | Signals for every token entry in this run, as JSON, such as `{"repository":"kestrel/storefront","pull_request":12}`. The names are those of `--signal`. |
| `TC_HOME` | The folder for the settings file and the local database. |
| `CLAUDE_CONFIG_DIR`, `CODEX_HOME` | Where Claude Code and Codex keep their session files, as the agents themselves read them. |
