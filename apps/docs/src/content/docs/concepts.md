---
title: "How it works"
description: "How Token Controller connects what your AI agents used with what your providers billed: usage, bills, and the tags that join them, built up in three steps with one worked month."
---

When your team uses AI agents, you may get bills from more than one provider, or two bills from the same one, such as an API account and a seat plan. You may want to know what each bill paid for, which project or client it belongs to, and whether it matches what your agents used. This page builds up how Token Controller answers that, in three small steps. Every term is in the [glossary](/glossary), and the formulas are on the [data model](/reference/data-model) page.

## Step 1: usage {#usage}

<div class="diagram flow"><svg viewBox="0 0 720 190" role="img" aria-label="A collector sends usage to a usage endpoint; each model call becomes a message; tokens times the price table give its cost"><defs><marker id="ar-usage" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="ah" d="M0,0 L10,5 L0,10 z"/></marker></defs><rect class="src" x="0" y="20" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="42" text-anchor="middle">Collectors</text><text class="sub" x="75.0" y="60" text-anchor="middle">tc · mod · Action</text><text class="sub" x="75.0" y="76" text-anchor="middle">your own script</text><rect class="src" x="190" y="20" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="42" text-anchor="middle">Usage endpoints</text><text class="sub" x="265.0" y="60" text-anchor="middle">API · OpenTelemetry</text><text class="sub" x="265.0" y="76" text-anchor="middle">CSV upload</text><rect class="rec" x="380" y="20" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="42" text-anchor="middle">Message</text><text class="sub" x="455.0" y="60" text-anchor="middle">person · agent · model</text><text class="sub" x="455.0" y="76" text-anchor="middle">time · tokens by kind</text><rect class="out" x="570" y="20" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="42" text-anchor="middle">Cost</text><text class="sub" x="645.0" y="60" text-anchor="middle">tokens × price</text><text class="sub" x="645.0" y="76" text-anchor="middle">per kind</text><path class="ln" d="M150,57 L188,57" marker-end="url(#ar-usage)"/><text class="lbl" x="169.0" y="51.0" text-anchor="middle">send</text><path class="ln" d="M340,57 L378,57" marker-end="url(#ar-usage)"/><text class="lbl" x="359.0" y="51.0" text-anchor="middle">store</text><path class="ln" d="M530,57 L568,57" marker-end="url(#ar-usage)"/><rect class="rec" x="570" y="112" width="150" height="58" rx="8"/><text class="hd" x="645.0" y="134" text-anchor="middle">Price table</text><text class="sub" x="645.0" y="152" text-anchor="middle">provider · model · kind</text><path class="ln" d="M645,112 L645,96" marker-end="url(#ar-usage)"/></svg></div>

Your agents call models, and each call uses tokens. A collector, such as [tc](/collect/cli), the [Claude Code mod](/collect/claude-code) or your own script, gathers these calls where the agents run and sends them to a usage endpoint: the [token entries API](/collect/token-entries), the [OpenTelemetry endpoint](/collect/telemetry) or a CSV upload. Token Controller first stores what it received, as it arrived, and then reads it. Nothing that arrives is thrown away, except content text such as prompts, which is removed unless your organization switches keeping it on.

Each model call becomes a message: who ran it, which agent, which model, when, and how many tokens of each kind. Everything else that came along, such as the branch, the repository or how the agent was signed in, is kept as a signal. Signals may help later, when you want to say what the work was for.

A message's cost is its tokens times the [price table](/account/price-table), kind by kind:

| | Tokens | Price per million | Cost |
| --- | ---: | ---: | ---: |
| Input | 12,000 | 3.00 | 0.036 |
| Output | 2,000 | 15.00 | 0.030 |
| Cache read | 80,000 | 0.30 | 0.024 |
| Message | | | 0.090 |

The price table knows the provider, the model and the kind. The provider is who charges you, which is not always who made the model, so Token Controller never guesses it from the model's name. When it cannot price a message for certain, the message shows as not priced yet, with its token counts, and never as zero.

## Step 2: bills {#bills}

<div class="diagram flow"><svg viewBox="0 0 720 114" role="img" aria-label="A bill source brings bills; each bill has an amount, a period and one contract"><defs><marker id="ar-bills" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="ah" d="M0,0 L10,5 L0,10 z"/></marker></defs><rect class="src" x="0" y="20" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="42" text-anchor="middle">Bill sources</text><text class="sub" x="75.0" y="60" text-anchor="middle">billing APIs</text><text class="sub" x="75.0" y="76" text-anchor="middle">CSV upload</text><rect class="rec" x="380" y="20" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="42" text-anchor="middle">Bill</text><text class="sub" x="455.0" y="60" text-anchor="middle">amount · period</text><text class="sub" x="455.0" y="76" text-anchor="middle">one contract</text><path class="ln" d="M150,57 L378,57" marker-end="url(#ar-bills)"/><text class="lbl" x="264.0" y="51.0" text-anchor="middle">bring</text></svg></div>

Bills come in on their own, from a provider's billing API or a CSV upload. Each bill belongs to one contract: one thing a provider charges you under. A contract may be metered, like an API account billed per token, or flat, like a seat plan with a fixed fee and maybe some extra usage on top. You may have two contracts with the same provider, and each one gets its own bills. Connecting or uploading a bill creates its contract, so there is nothing to set up first. See [Bills](/account/bills).

## Step 3: tags connect the two {#tags}

<div class="diagram flow"><svg viewBox="0 0 720 350" role="img" aria-label="Tags from tag sources go on messages, by a rule on signals, a default or by hand, and on bills as one contract each; the check compares a bill with the messages that carry the same contract"><defs><marker id="ar-tags" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="ah" d="M0,0 L10,5 L0,10 z"/></marker></defs><rect class="src" x="0" y="20" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="42" text-anchor="middle">Usage</text><text class="sub" x="75.0" y="60" text-anchor="middle">messages, from</text><text class="sub" x="75.0" y="76" text-anchor="middle">step 1</text><rect class="rec" x="380" y="20" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="42" text-anchor="middle">Message</text><text class="sub" x="455.0" y="60" text-anchor="middle">signals: branch,</text><text class="sub" x="455.0" y="76" text-anchor="middle">sign-in, anything</text><rect class="src" x="0" y="140" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="162" text-anchor="middle">Tag sources</text><text class="sub" x="75.0" y="180" text-anchor="middle">Jira · CRM · a list</text><text class="sub" x="75.0" y="196" text-anchor="middle">by hand</text><rect class="tag" x="190" y="140" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="162" text-anchor="middle">Tags</text><text class="sub" x="265.0" y="180" text-anchor="middle">project · feature</text><text class="sub" x="265.0" y="196" text-anchor="middle">client · contract</text><rect class="src" x="0" y="260" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="282" text-anchor="middle">Bills</text><text class="sub" x="75.0" y="300" text-anchor="middle">from step 2</text><rect class="rec" x="380" y="260" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="282" text-anchor="middle">Bill</text><text class="sub" x="455.0" y="300" text-anchor="middle">one contract tag</text><rect class="out" x="570" y="140" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="162" text-anchor="middle">The check</text><text class="sub" x="645.0" y="180" text-anchor="middle">bill = messages</text><text class="sub" x="645.0" y="196" text-anchor="middle">+ unexplained</text><path class="ln" d="M150,57 L378,57" marker-end="url(#ar-tags)"/><path class="ln" d="M150,297 L378,297" marker-end="url(#ar-tags)"/><path class="ln" d="M150,177 L188,177" marker-end="url(#ar-tags)"/><text class="lbl" x="169.0" y="171.0" text-anchor="middle">bring</text><path class="ln" d="M300,140 L400,96" marker-end="url(#ar-tags)"/><text class="lbl" x="300" y="112" text-anchor="middle">by a rule, a default or by hand</text><path class="ln" d="M300,214 L400,258" marker-end="url(#ar-tags)"/><text class="lbl" x="300" y="248" text-anchor="middle">one contract per bill</text><path class="ln" d="M530,57 L600,138" marker-end="url(#ar-tags)"/><text class="lbl" x="600" y="112" text-anchor="middle">messages with the contract</text><path class="ln" d="M530,297 L600,216" marker-end="url(#ar-tags)"/><text class="lbl" x="590" y="248" text-anchor="middle">amount</text></svg></div>

So far, messages and bills know nothing about each other. Tags connect them. A tag says what something was for, such as `project: shop`, `feature: KD-1` or `client: acme`. Tag values may come from a tag source such as Jira or a CRM, from a list, or be typed by hand.

Tags go on messages afterwards, by a [rule](/attribute/rules) that reads a signal ("repository `kestrel/shop` is project Shop"), by a default ("feature KD-1 belongs to project Shop"), or by hand in the [review list](/attribute/review). They also run back in time, so a rule you add today may tag last month's messages too.

One tag is built in for the money side: `contract`. A bill has exactly one contract, and a message gets at most one, from the first of these that applies:

1. Someone set it by hand, or the collector sent it.
2. A rule matched a signal, such as "signed in with a plan" or "signed in with an API key". Token Controller ships these rules for the agents it knows.
3. The message's project brings a contract by default.
4. The message's provider has only one contract.

With the contract on both, a bill can be checked against its own messages. For a metered contract, the bill should equal the cost of the messages with that contract, plus what no message explains, such as usage in the provider's own chat app. For a flat contract, the bill is the fee plus any extra usage. Next to every check you also see the messages of the same provider that have no contract yet, and those that got their contract only because the provider has one, so a check cannot look right by leaving messages out.

## The whole picture {#picture}

<div class="diagram flow"><svg viewBox="0 0 720 350" role="img" aria-label="The full picture: usage sources bring messages, tag sources bring tags, bill sources bring bills; tags go on messages and bills; the price table gives cost; reports show cost by any tag; the check compares each bill with the messages under its contract"><defs><marker id="ar-full" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="ah" d="M0,0 L10,5 L0,10 z"/></marker></defs><rect class="src" x="0" y="20" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="42" text-anchor="middle">Usage sources</text><text class="sub" x="75.0" y="60" text-anchor="middle">collectors send to</text><text class="sub" x="75.0" y="76" text-anchor="middle">usage endpoints</text><rect class="rec" x="190" y="20" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="42" text-anchor="middle">Message</text><text class="sub" x="265.0" y="60" text-anchor="middle">tokens by kind</text><text class="sub" x="265.0" y="76" text-anchor="middle">signals</text><rect class="out" x="380" y="20" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="42" text-anchor="middle">Cost</text><text class="sub" x="455.0" y="60" text-anchor="middle">tokens × price</text><text class="sub" x="455.0" y="76" text-anchor="middle">per kind</text><rect class="out" x="570" y="20" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="42" text-anchor="middle">Reports</text><text class="sub" x="645.0" y="60" text-anchor="middle">cost by any tag</text><rect class="src" x="0" y="140" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="162" text-anchor="middle">Tag sources</text><text class="sub" x="75.0" y="180" text-anchor="middle">Jira · CRM · list</text><text class="sub" x="75.0" y="196" text-anchor="middle">by hand</text><rect class="tag" x="190" y="140" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="162" text-anchor="middle">Tags</text><text class="sub" x="265.0" y="180" text-anchor="middle">project · feature</text><text class="sub" x="265.0" y="196" text-anchor="middle">client · contract</text><rect class="rec" x="380" y="140" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="162" text-anchor="middle">Price table</text><text class="sub" x="455.0" y="180" text-anchor="middle">ours, or the</text><text class="sub" x="455.0" y="196" text-anchor="middle">contract&#x27;s own</text><rect class="src" x="0" y="260" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="282" text-anchor="middle">Bill sources</text><text class="sub" x="75.0" y="300" text-anchor="middle">billing APIs</text><text class="sub" x="75.0" y="316" text-anchor="middle">CSV upload</text><rect class="rec" x="190" y="260" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="282" text-anchor="middle">Bill</text><text class="sub" x="265.0" y="300" text-anchor="middle">amount · period</text><rect class="out" x="570" y="200" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="222" text-anchor="middle">The check</text><text class="sub" x="645.0" y="240" text-anchor="middle">bill = messages</text><text class="sub" x="645.0" y="256" text-anchor="middle">+ unexplained</text><path class="ln" d="M150,57 L188,57" marker-end="url(#ar-full)"/><path class="ln" d="M340,57 L378,57" marker-end="url(#ar-full)"/><path class="ln" d="M530,57 L568,57" marker-end="url(#ar-full)"/><path class="ln" d="M150,177 L188,177" marker-end="url(#ar-full)"/><path class="ln" d="M150,297 L188,297" marker-end="url(#ar-full)"/><path class="ln" d="M265,140 L265,96" marker-end="url(#ar-full)"/><text class="lbl" x="290" y="122" text-anchor="middle">rules</text><path class="ln" d="M265,214 L265,258" marker-end="url(#ar-full)"/><text class="lbl" x="298" y="240" text-anchor="middle">contract</text><path class="ln" d="M455,140 L455,96" marker-end="url(#ar-full)"/><path class="ln" d="M530,80 L600,198" marker-end="url(#ar-full)"/><path class="ln" d="M340,297 L568,250" marker-end="url(#ar-full)"/></svg></div>

Usage, tags and bills each come from their own sources and load in any order. The price table turns messages into cost. Reports show that cost by any tag, and the check compares each bill with the messages under its contract.

## A worked month {#month}

A small agency works in March with three contracts: a Claude seat plan `claude-team` (flat, fee 200), an Anthropic API account `anthropic-api` (metered) and an OpenAI API account `openai-api` (metered). Features KD-1 and KD-2 have the default project Shop, and Shop has the default client Acme. Mara and Tom have the attribute team web, and team web has the default department engineering. Tom holds the service token of the review bot.

| Session | Person | Agent | Contract | Set by | Cost |
| --- | --- | --- | --- | --- | ---: |
| S1 | Mara | Claude Code | `claude-team` | rule: signed in with a plan | 400 |
| S2 | Mara | Codex | `openai-api` | the provider's only contract | 300 |
| S3 | Tom | review bot | `anthropic-api` | rule: signed in with an API key | 100 |
| S4 | Tom | Claude Code | `anthropic-api` | rule: signed in with an API key | 150 |
| All messages | | | | | 950 |

The same sessions by what the work was for:

| Session | Split | Cost | Deepest tag | Label |
| --- | ---: | ---: | --- | --- |
| S1 | 100 % | 400 | KD-1 | direct |
| S2 | 60 % | 180 | KD-1 | direct |
| S2 | 40 % | 120 | KD-2 | direct |
| S3 | 100 % | 100 | Shop | project overhead |
| S4 | 100 % | 150 | none | organization overhead |

The three bills and their checks:

| Contract | Bill | Messages | Unexplained | Fee | Extra usage | Covered by the plan |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `anthropic-api`, metered | 270 | 250 | 20 | | | |
| `openai-api`, metered | 300 | 300 | 0 | | | |
| `claude-team`, flat | 250 | 400 | | 200 | 50 | 350 |

Check, metered: 250 + 20 − 270 = 0 and 300 + 0 − 300 = 0. Check, flat: 200 + 50 − 250 = 0. Beside the checks: no Anthropic message without a contract, and the 300 of OpenAI messages that got their contract from the fallback.

Reports show cost at the price table: 950 in all, of which 400 is marked "on a plan" because the seat plan's fee already paid for most of it. The bills add up to 820. They differ from 950 because the seat plan covered 350 of its 400 for a fee of 200, and the Anthropic API bill has 20 that no message explains: 950 − 350 + 200 + 20 = 820.

Every slice of the same 950: person Mara 700, Tom 250. Feature KD-1 580, KD-2 120, Shop only 100, none 150. Label direct 700, project overhead 100, organization overhead 150. Project Shop 800, none 150. Client Acme 800. Team web 950. Department engineering 950. Contract `claude-team` 400, `openai-api` 300, `anthropic-api` 250.

## Who and how much {#splits}

A session may have worked on more than one thing. A [split](/attribute/rules#splits) puts fractions of it on different tags, such as 60 % on KD-1 and 40 % on KD-2. People carry [attributes](/account/attributes), such as their team, so you may also ask what a team or a department spent. A split with a feature is direct, with a project and no feature project overhead, with neither organization overhead. It always keeps its person.

Every report uses one formula: the cost of a filter, which is any set of tags and attributes, over a date range. For one tag name the values add up to the total, so cost per project adds up to the whole. Two filters on different names, such as `team: web` and `project: shop`, may count the same money and are never added up. See [Reports](/report/reports).

## Figures as of a date {#log}

Your organization keeps everything as an append-only log. A correction is a new entry that reverses an older one, and the older one stays visible. Every figure is computed when you look, as of a date: today for the current figures, or the [cutoff](/account/cutoff) for the figures you already gave to finance. A rule or a contract that runs back in time never changes figures as of the cutoff.

## Plans and deviation {#plans}

On top of cost you may plan. An [estimate](/account/estimates) is what described work may cost, as bands with a median. Accepting one makes it the plan for its tags, spread over its days. The deviation is plan minus cost, for any filter and date range, and positive is favourable. Plans read cost and never change it. See [Deviation and forecast](/report/deviation).

## Agents {#agents}

An agent is software that calls a model on a person's behalf. Token Controller keeps no record of agents, only their messages, each with its agent and an optional agent name. A person typing in a provider's own chat app is not an agent; that usage is what a bill has that no message explains. What is kept and what is removed is on [Security and privacy](/security).

## Local first {#local}

The [CLI](/collect/cli) is useful with no service connected, including an export for a spreadsheet. The local tool and the service talk over the documented [API](/reference/api), so the local tool can connect to any compatible service, or none.
