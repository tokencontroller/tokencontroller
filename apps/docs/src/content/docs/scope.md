---
title: "Scope"
description: "Everything Token Controller does, everything it will not do, and why. The scope is fixed."
---

The scope is fixed: what is not listed here will not be built.

## What is in {#in}

| Area | What you get |
| --- | --- |
| Collect | The [Claude Code mod](/collect/claude-code) for macOS, Windows and Linux, in the terminal and in the desktop app. The [CLI](/collect/cli), which works with no service connected and exports for a spreadsheet. [Telemetry push](/collect/telemetry) from Claude Code, Codex, Gemini CLI, Copilot CLI and Cursor Enterprise. The [GitHub Action](/collect/github-action) for CI agents. The [token entries endpoint](/collect/token-entries) for any other agent. [Manual entry and CSV import](/collect/manual) in the web app. |
| Attribute | [Tags](/account/tags) on every token entry, with a tag map for the cost center and the cost object. [Rules](/attribute/rules) on branch, repository, pull request and ticket keys. Tags from a repository's config file and from telemetry. Sessions split into [segments](/attribute/rules#sessions) when the branch or directory changes. [Review and submit](/attribute/review) of token sheets. An optional [classifier](/attribute/classifier) on your own machine, with one reference script that uses an open local model. |
| Close and report | [Bills](/account/bills) from the Anthropic and OpenAI admin APIs, as CSV or typed in. [Approval](/account/cadence#approval) and reject. The [monthly close](/account/close) and the [cost sheet](/account/close#cost-sheet). [Adjustments](/account/close#adjustments) after close. [Reports](/report/reports) on token budget versus actual, agent harness, agent name, model and outliers. [Exports](/report/exports): CSV from every table, a read-only reporting API, and a full raw export. |
| Plan | [Token budgets](/account/token-budgets) on any tag filter: people, tickets, projects, teams, clients. Token estimates from past cost. A [cadence](/account/cadence) for token sheets, with reminders when one is late. |
| Estimate | The [Estimates API](https://tokencontroller.com/estimates), coming soon: send a description of work and get back a low, likely and high cost, from what similar work actually cost. It needs no token entries of your own. Recorded work shows each estimate next to its actual cost. |
| Connect | Ticket sync with [Jira Cloud](/account/projects#jira), [Azure DevOps](/account/projects#azure-devops) and [GitHub Issues](/account/projects#github-issues). [Sign-in](/account/organizations#sign-in) by magic link. [OIDC and SCIM](/self-hosting/enterprise#oidc-scim) on the Enterprise plan. Bills from Bedrock, Vertex and Foundry as [CSV](/account/bills#csv), or through a connector on an Enterprise plan contract. |

## What is out {#out}

| Area | Not built, and why |
| --- | --- |
| Apps | No mobile app. No native desktop app, including menu bar and tray apps; the Claude Code mod shows the same numbers inside Claude Code. No Slack bots, browser extensions or embedded dashboards. The web app is readable on a phone. |
| Spending | No spend alerts and no spend caps: your provider offers both. A token budget is a plan to compare against and never stops an agent. The only reminder is for a late token sheet. |
| Agent estimates | Agents do not estimate their own cost and do not explain variances. Agent self-estimates match actual cost poorly, with a correlation of about 0.4. An agent that needs an estimate asks the Estimates API, which answers from what similar work actually cost. |
| Classifier | No bundled model. The classifier is a program you run on your own machine. |
| Session files | Session files are read for Claude Code and Codex only. Other agents report by telemetry or through the token entries endpoint. |
| Ticket systems | Only Jira Cloud, Azure DevOps and GitHub Issues. Tickets from other systems can be created by hand. |
| Enterprise only | Cloud bill connectors and telemetry passthrough to your own collector exist only on an Enterprise plan contract. |
| Organization | One account is one organization. An agency with a separate AI account per client runs one organization per client. The Team plan stops at 150 members. See [Plans and limits](/plans). |
| Privacy modes | No aggregate-only mode, such as the team totals works councils sometimes ask for. People managers see cost per person within their scope, and controllers see all of it. Clients are billed for work per person, and the providers' own admin tools already show cost per person. |
| Local tool | No sync between one person's machines. Each machine submits its token entries on its own. |
| Flat fees | Only metered usage is accounted for: tokens and per-call fees. Seats, subscriptions and other flat fees stay off the bill and off the cost sheet. What the agent harness does not report shows as residual. |
| Org chart | No team tree. Teams and departments are tags on people, one value per tag name. |
| Chat | Usage in a provider's own chat app is not an agent's and never becomes a token entry. It shows as residual. |
| Your clients | Token Controller ends at the cost sheet. It does not invoice your clients, add markups or bill on your behalf. Pass-through happens in your own invoicing tool. |
| Open source | Issues on GitHub are open. Pull requests from outside are not accepted. See [Open source and licenses](/reference/open-source). |

## Common questions {#faq}

| Question | Answer |
| --- | --- |
| Do I need the hosted service? | No. The CLI is fully useful on its own; connect it to a service when you want to share numbers. See [Local first](/concepts#local). |
| Is it only for coding agents? | No. Any agent that can report home, such as a support agent, sends token entries through the [token entries endpoint](/collect/token-entries). |
