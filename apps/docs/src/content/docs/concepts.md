---
title: "How it works"
description: "The ideas Token Controller builds on: two sides that match, cost booked twice, tags, cost kinds and the budgeting loop."
---

Token Controller is cost accounting for AI agents, with management control on top. Every term below is defined in the [glossary](/glossary).

## Two sides that must match {#two-sides}

As in double-entry bookkeeping, there are two sides, and they must add up to the same total.

| Side | What it is |
| --- | --- |
| Token entries | One per agent session: tokens, cost, model, agent harness, start and end time, a summary, and the ticket or project it was for. Token Controller collects these. |
| Bills | One figure per provider and period for the whole organization, with every client and project mixed together. Only metered usage is on it: tokens and per-call fees. Seats and other flat fees are not. |

Once the token entries add up to the bill, the bill can be split down to the work that caused it. The check runs per bill, and the organization total adds them up. What no token entry explains, such as usage in the provider's own chat app, shows as **residual**.

Some token entries arrive **automatically**: from cloud agents, CI agents, a support agent that answers email, and telemetry. Telemetry brings the money but not the ticket or the summary. Those come from the person who ran the session, on a [token sheet](/account/cadence), the way hours go on a timesheet. When both describe the same session, they are [joined and counted once](/collect/telemetry).

On a seat plan (Claude Team, Max, Pro) there is no usage bill. Token Controller then shows what the usage would have cost at API prices, labelled as an estimate on every figure, and every report can switch to the **share view**: percentages of the period instead of money. See [what the close reconciles against](/account/bills#what).

## Every cost is booked twice {#two-trees}

A timesheet books hours both to a person and to a ticket. Token Controller does the same with cost. Both come from [tags](/account/tags) on the token entry, such as `person: mara`, `ticket: KD-214` or `team: web`.

| Booked to | Answers | How it is built |
| --- | --- | --- |
| Cost center | Who answers for it? | Every token entry has exactly one agent manager: you for your own sessions, a tech lead for the CI automations they set up. By default the person is the cost center. People carry tags such as `team: web`, which their token entries inherit. An organization where teams answer for money maps the cost center to `team`. |
| Cost object | What did it pay for? | The first rung of the ladder with a value: by default a ticket, then a project, then the organization. Tickets can sit inside other tickets, so a project's cost is the sum of its tickets. A client is a tag on projects, such as `client: acme`; an organization can add it as a rung. |

The two are separate on purpose. A developer can work on three projects for three project managers and still be one cost center. Tech leads and heads of department read cost by person and team. Project managers, directors of delivery and account managers read it by ticket, project and client, whoever caused it.

<div class="note"><b>Example.</b> A PR review bot reviews a pull request for ticket KD-214. The cost sits with the tech lead who is the bot's agent manager (cost center) and shows on KD-214 as review cost (cost object). The ticket's assignee is not charged for it, but a ticket with expensive reviews is visible on its report.</div>

What has no ticket and no project sits on the organization as organization overhead, still with its person. A token entry without an agent manager shows as an error to people managers until someone takes it.

## Tags {#tags}

Beyond person, ticket and project, an organization adds the tags it needs: `team`, `client`, `feature`, `purpose`. A token entry has one value per tag name, so cost by team or by client always adds up. A report is a saved tag filter, a token budget is money for a tag filter, and a people manager is assigned the filters they look after. Most tags come in on their own: from the person, the project, rules, a repository's config file or telemetry. See [Tags and the tag map](/account/tags).

<div class="note"><b>Example.</b> An agency tags people with their team and projects with their client. Without anyone typing a tag, the account manager for Acme reads cost per team for <code>client: acme</code>, and the Web lead reads cost per client for <code>team: web</code>.</div>

## Agents {#agents}

An agent is software that calls a model on a person's behalf and so costs money. A person typing in a provider's own chat app is not an agent; that usage shows as residual. Token Controller keeps no record of agents, only of the cost they cause. Each token entry names the agent harness (Claude Code, Codex), the model, and an optional agent name such as "review bot", so the same automation on several projects can be totalled.

## Cost kinds {#cost-kinds}

Nobody sets the cost kind; it follows from the ticket and project tags. A cost on a ticket is **direct**, on a project but no ticket it is **project overhead** (research, discovery, setup), and on no project it is **organization overhead** (dependency updates, internal tooling, learning, an untagged session). See [the cost kind](/account/tags#kinds).

## Every token, accounted for {#accounted}

The whole bill is always accounted for. Every token entry has a person, so everything Token Controller receives is attributed, and whatever no token entry explains shows as residual, never hidden. Tags only add detail. How far to go is your choice: reports show how much cost is direct, project overhead and organization overhead, and [rules](/attribute/rules) move it towards tickets month by month. Every figure says what it is: billed, a list-price estimate, or an unknown model.

## The loop {#loop}

On top of the accounting sits the classic budgeting loop: plan, record, reconcile, learn.

<ol class="steps">
  <li><b>Plan.</b><p>A <a href="/account/token-budgets">token budget</a> on a person (for example 1,500 EUR a month), a ticket, a project, a team or any other tag filter, for a period. You type it, or accept a token estimate: what similar past work cost, found by comparing the ticket with the summaries of past token entries.</p></li>
  <li><b>Record.</b><p>Token entries come in, automatically or on token sheets.</p></li>
  <li><b>Reconcile.</b><p>Rules tag each token entry with its ticket or project, people review and submit, the bills come in, both sides are checked against each other, and the month is closed.</p></li>
  <li><b>Learn.</b><p>Actual against token budget, per person, ticket, project or any tag. An overrun may be a problem. An underrun may mean someone can teach the others, or that the next token estimate should be lower. History feeds the next token estimate, so a new client project can be quoted from what similar work cost before.</p></li>
</ol>

Every part of the loop is optional: you can only collect token entries and match them to the bill, and add token budgets later. A token budget never stops spending.

## Where Token Controller ends {#ends}

At the [cost sheet](/account/close#cost-sheet): the locked account of one closed period. Charging a client for what is on it (pass-through) happens in your own invoicing tool.

## Any provider, any agent {#sources}

Providers charge through subscriptions, seats, usage at API rates, API keys, cloud marketplaces (Bedrock, Vertex, Foundry) and tools that resell model usage. Token Controller treats them all the same way; a bill exists for the metered ones. Every organization has one reporting currency; a bill in another currency is booked at the exchange rate of its date, and any report can be shown in another currency.

Coding agents came first because their session files sit on the developer's machine. Any agent that can report home works, including remote and support agents.

## Local first {#local}

The [CLI](/collect/cli) is fully useful with no service connected, including an export for reporting by hand in a spreadsheet. The local tool and the service talk over a documented protocol, so the local tool can connect to any compatible service, or none. Transcripts never leave the machine.
