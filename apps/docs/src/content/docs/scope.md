---
title: "Scope"
description: "Everything Token Controller does and everything it will not do. The scope is fixed."
prev:
  link: "/plans"
  label: "Plans and limits"
next:
  link: "/concepts"
  label: "Concepts"
---

This page lists everything Token Controller does and everything it will not do. The list is fixed: what is not here will not be built.

## What is in {#in}

| Area | What you get |
| --- | --- |
| Collect | The [Claude Code mod](/collect/claude-code) for macOS, Windows and Linux, in the terminal and in the desktop app. The [CLI](/collect/cli), which works with no service connected and exports for a spreadsheet. [Telemetry push](/collect/telemetry) from Claude Code, Codex, Gemini CLI, Copilot CLI and Cursor Enterprise. The [GitHub Action](/collect/github-action) for CI agents. The [token entries endpoint](/collect/token-entries) for any other agent. Manual entry and CSV import in the web app. |
| Attribute | [Tags](/account/tags) on every token entry, with a tag map for the cost center and the cost object. [Rules](/attribute/rules) on branch, repository, pull request and ticket keys. Tags from a repository's config file and from telemetry. Sessions split into segments when the branch or directory changes. [Review and submit](/attribute/review) of token sheets. An optional [classifier](/attribute/classifier) on your own machine, with one reference script that uses an open local model. |
| Close and report | [Bills](/account/bills) from the Anthropic and OpenAI admin APIs, as CSV or typed in. [Approval](/account/approval) and reject. The [monthly close](/account/close) and the [cost sheet](/report/cost-sheet). [Adjustments](/account/adjustments) after close. Reports on token budget versus actual, agent harness, agent name, model and outliers. [Exports](/report/exports): CSV from every table, a read-only reporting API, and a full raw export. |
| Plan | [Token budgets](/account/token-budgets) on any tag filter: people, tickets, projects, teams, clients. Token forecasts from past cost. A [cadence](/account/cadence) for token sheets, with reminders when one is late. |
| Connect | Ticket sync with [Jira Cloud](/integrations/jira), [Azure DevOps](/integrations/azure-devops) and [GitHub Issues](/integrations/github-issues). Sign-in by magic link. OIDC and SCIM on the Enterprise plan. Bills from Bedrock, Vertex and Foundry as CSV, or through a connector on an Enterprise plan contract. |

## What is out {#out}

| Area | Not built |
| --- | --- |
| Apps | No mobile app. No native desktop app, including menu bar and tray apps; the Claude Code mod shows the same numbers inside Claude Code. No Slack bots, browser extensions or embedded dashboards. The web app is readable on a phone. |
| Spending | No spend alerts and no spend caps. Your provider offers both. A token budget never stops spending. The only reminder is for a late token sheet. |
| Agent estimates | Agents do not estimate their own cost, do not explain variances, and get no estimates fed back. Agent self-estimates match actual cost poorly, with a correlation of about 0.4. |
| Classifier | No bundled model. The classifier is a program you run on your own machine. |
| Session files | Session files are read for Claude Code and Codex only. Other agents report by telemetry or through the token entries endpoint. |
| Ticket systems | Only Jira Cloud, Azure DevOps and GitHub Issues. Tickets from other systems can be created by hand. |
| Enterprise only | Cloud bill connectors and telemetry passthrough to your own collector exist only on an Enterprise plan contract. |
| Organization | One account is one organization. An agency with a separate AI account per client runs one organization per client. The Team plan stops at 150 members. |
| Privacy modes | No aggregate-only mode. People managers see cost per person within their scope, and controllers see all of it. |
| Local tool | No sync between one person's machines. Each machine submits its token entries on its own. |
| Flat fees | Only metered usage is accounted for: tokens and per-call fees. Seats, subscriptions and other flat fees stay off the bill and off the cost sheet. What the agent harness does not report shows as residual. |
| Org chart | No team tree. Teams and departments are tags on people, one value per tag name. |
| Chat | Usage in a provider's own chat app is not an agent's and never becomes a token entry. It shows as residual. |
| Your clients | Token Controller ends at the cost sheet. It does not invoice your clients, add markups or bill on your behalf. Pass-through happens in your own invoicing tool. |
| Open source | Issues on GitHub are open. Pull requests from outside are not accepted. |

## Where to next {#next}
