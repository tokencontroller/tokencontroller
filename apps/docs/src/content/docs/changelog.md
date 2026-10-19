---
title: "Changelog"
description: "Every release of Token Controller, newest first."
---

Every release of the service, the Claude Code mod and the CLI. The hosted service runs the latest version. A self-hosted instance upgrades by pulling the new image; see [Upgrades and backups](/self-hosting#upgrades).

## Versions {#versions}

Version numbers follow semantic versioning. A new major version can change the API or the database. Read its entry before you upgrade a self-hosted instance. The Docker image is tagged with each version, such as `ghcr.io/tokencontroller/tokencontroller:1.0`.

## 1.0.0 {#v1-0-0}

First release.

- The [Claude Code mod](/collect/claude-code) and the [CLI](/collect/cli), `tc`, which read session files, split sessions with your rules and submit.
- [Telemetry ingest](/collect/telemetry) over OpenTelemetry for Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor.
- The [token entries endpoint](/collect/token-entries) for any other agent, and [messages by hand or from a CSV file](/collect/manual).
- The [GitHub Action](/collect/github-action) for agents that run in CI, such as a PR review bot.
- [Rules](/attribute/rules) over signals that split sessions onto features and projects.
- [Bills](/account/bills) from Anthropic and OpenAI, and any bill by CSV.
- Feature sync from [Jira Cloud](/account/tags#jira).
- [Estimates and plans](/account/estimates), with [deviation and forecast](/report/deviation).
- [Submit and approval](/account/submit), the [cutoff](/account/cutoff) and the [price table](/account/price-table).
- [Reports](/report/reports) as the cost of any filter of [tags](/account/tags) and [attributes](/account/attributes), as of any date, and the [agent harness](/report/reports#agent-harness) report.
- [CSV exports](/report/exports), the [reporting API](/report/exports#reporting-api) and the [raw export](/report/exports#raw-export).
- The [self-hosted Docker image](/self-hosting): the same product as the hosted Team plan, from the same open code.
