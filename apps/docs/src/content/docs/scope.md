---
title: "Scope"
description: "Everything Token Controller does, everything it will not do, and why. The scope is fixed."
---

The scope is fixed: what is not listed here will not be built.

## What is in {#in}

| Area | What you get |
| --- | --- |
| Collect | Every model call as a [message](/collect#message) with its token counts by kind. The [Claude Code mod](/collect/claude-code) for macOS, Windows and Linux, in the terminal and in the desktop app. The [CLI](/collect/cli), which works with no service connected and exports for a spreadsheet. [Telemetry push](/collect/telemetry) from Claude Code, Codex, Gemini CLI, Copilot CLI and Cursor Enterprise. The [GitHub Action](/collect/github-action) for CI agents. The [token entries endpoint](/collect/token-entries) for any other agent. [Manual entry and CSV import](/collect/manual) in the web app. |
| Assign | [Tag values](/account/tags) as records with defaults: project, feature, client and your own names. [Attributes](/account/attributes) on people. [Rules](/attribute/rules) over signals: branch, repository, pull request, keys, config file values, telemetry attributes. [Splits](/attribute/rules#splits) by message, by fraction or equal. [Check and submit](/attribute/review), with remember. |
| Reconcile and report | The [price table](/account/price-table) as the only cost basis. [Bills](/account/bills) from the Anthropic and OpenAI admin APIs, as CSV or typed in, reconciled on their own range. [Approval](/account/submit#approval) and reject. One [cutoff](/account/cutoff) per organization, with reversals instead of edits. [Reports](/report/reports): the cost of any filter, grouped by any tag or attribute name, as of any date. [Exports](/report/exports): CSV from every report, a read-only reporting API, and a raw export of the event log. |
| Plan | [Estimates and plans](/account/estimates) on any filter: features, projects, people through shares, teams, clients. [Deviation](/report/deviation) with labels and a forecast as a report. A reminder to submit before the cutoff moves. |
| Estimate | The [Estimates API](/reference/api/estimates): send a description of work and get back a probability distribution of its cost, with its median, from what similar work actually cost. It needs no messages of your own. Recorded work shows each estimate next to its cost. |
| Connect | Feature sync from [Jira Cloud](/account/tags#jira). [Sign-in](/account/organizations#sign-in) by magic link. [OIDC and SCIM](/self-hosting/enterprise#oidc-scim) on the Enterprise plan. Bills from Bedrock, Vertex and Foundry as [CSV](/account/bills#csv), or through a connector on an Enterprise plan contract. |

## What is out {#out}

| Area | Not built, and why |
| --- | --- |
| Apps | No mobile app. No native desktop app, including menu bar and tray apps; the Claude Code mod shows the same numbers inside Claude Code. No Slack bots, browser extensions or embedded dashboards. The web app is readable on a phone. |
| Spending | No spend alerts and no spend caps: your provider offers both. A plan is compared against cost and never stops an agent. The only reminder is an org manager asking everyone to submit. |
| Agent estimates | Agents do not estimate their own cost and do not explain variances. Agent self-estimates match actual cost poorly, with a correlation of about 0.4. An agent that needs an estimate asks the Estimates API, which answers from what similar work actually cost. |
| Session files | Session files are read for Claude Code and Codex only. Other agents report by telemetry or through the token entries endpoint. |
| Ticket systems | Only Jira Cloud. Features from other systems are created by hand or through the API. |
| Enterprise only | Cloud bill connectors and telemetry passthrough to your own collector exist only on an Enterprise plan contract. |
| Organization | One account is one organization. An agency with a separate AI account per client runs one organization per client. The Team plan stops at 150 members. See [Plans and limits](/plans). |
| Privacy modes | No aggregate-only mode, such as the team totals works councils sometimes ask for. People managers see cost per person inside their grants, and org managers see all of it. Clients are billed for work per person, and the providers' own admin tools already show cost per person. |
| Local tool | No sync between one person's machines. Each machine submits its messages on its own. |
| Flat fees | Only metered usage is on a bill: tokens and per-call fees. Seats, subscriptions and other flat fees are not. What the agent harness does not report is the line on the bill that no message explains. |
| Org chart | No team tree. Teams and departments are attributes on people, one value per name, with defaults for rollup. |
| Chat | Usage in a provider's own chat app is not an agent's and never becomes a message. It is what the bill has that no message explains. |
| Your clients | Token Controller ends at the cost of a filter. It does not invoice your clients or bill on your behalf. Pass-through happens in your own invoicing tool. |
| Open source | Issues on GitHub are open. Pull requests from outside are not accepted. See [Open source and licenses](/reference/open-source). |

## Common questions {#faq}

| Question | Answer |
| --- | --- |
| Do I need the hosted service? | No. The CLI is fully useful on its own; connect it to a service when you want to share numbers. See [Local first](/concepts#local). |
| Is it only for coding agents? | No. Any agent that can report home, such as a support agent, sends its messages through the [token entries endpoint](/collect/token-entries). |
