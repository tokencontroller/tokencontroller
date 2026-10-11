---
title: "Changelog"
description: "Every release of Token Controller, newest first."
---

Every release of the service, the Claude Code mod and the CLI. The hosted service runs the latest version. A self-hosted instance upgrades by pulling the new image; see [Upgrades and backups](/self-hosting/upgrades).

## Versions {#versions}

Version numbers follow semantic versioning. A new major version can change the API or the database. Read its entry before you upgrade a self-hosted instance. The Docker image is tagged with each version, such as `ghcr.io/tokencontroller/tokencontroller:1.0`.

## 1.0.0 {#v1-0-0}

First release.

- The [Claude Code mod](/collect/claude-code) and the [CLI](/collect/cli), `tc`, which read session files, place token entries with your rules and submit token sheets.
- [Telemetry ingest](/collect/telemetry) over OpenTelemetry for Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor.
- The [token entries endpoint](/collect/token-entries) for any other agent, and [token entries by hand or from a CSV file](/collect/manual).
- The [GitHub Action](/collect/github-action) for agents that run in CI, such as a PR review bot.
- [Rules](/attribute/rules), [repository mappings](/attribute/repositories) and [ticket key patterns](/attribute/ticket-keys) that place token entries on tickets and projects, and an optional [classifier](/attribute/classifier) on your own machine.
- [Bills](/account/bills) from Anthropic and OpenAI, and any bill by CSV.
- Ticket sync from [Jira Cloud](/integrations/jira), [Azure DevOps](/integrations/azure-devops) and [GitHub Issues](/integrations/github-issues).
- [Token budgets and token forecasts](/account/token-budgets), compared with actual cost.
- [Approval](/account/approval), the [close](/account/close), [adjustments](/account/adjustments) and the [cost sheet](/report/cost-sheet).
- [Reports](/report/reports) by person, team and any tag and by client, project and ticket, [tags](/account/tags) with a tag map for the cost center and the cost object, the [outliers](/report/outliers) report and the [agent harness](/report/agent-harness) report.
- [CSV exports](/report/exports), the [reporting API](/report/reporting-api) and the [raw export](/report/raw-export).
- The [self-hosted Docker image](/self-hosting), the same as the hosted Team plan.
