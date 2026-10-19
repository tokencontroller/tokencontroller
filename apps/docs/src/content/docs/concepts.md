---
title: "How it works"
description: "The ideas Token Controller builds on: two tiers, an event log read as of a date, one cost formula over tags and attributes, and plans compared with cost."
---

Token Controller is cost accounting for AI agents, with management control on top. Every term below is in the [glossary](/glossary); the formulas are in the [data model](/reference/data-model).

## Two tiers {#tiers}

| Tier | Answers | Records |
| --- | --- | --- |
| Cost accounting | What was spent, by whom, on what, reconciled to the provider's bill. Usable alone, for pass-through to clients. | Messages, bills, price table, tag values, attribute values, splits |
| Management control | What it was meant to cost, who carries it, where it is heading. | Estimates, accepted plans, shares, deviations, forecasts |

Management control reads cost accounting through one number, the cost of a filter, and never writes into it.

## An event log, read as of a date {#log}

An organization is an append-only log of events. Nothing is edited or deleted. A correction is a new event that reverses an older one; the older one stays visible and stops counting. Every figure is computed when you look, by reading the log up to a date: as of today gives the current figures, as of the [cutoff](/account/cutoff) gives the frozen ones. Every report shows its as-of date.

There are no months to close. One cutoff date per organization says up to where the figures are frozen; an org manager moves it, and only an org manager can add events dated before it.

## One message, one cost {#cost}

A message is one model call: token counts by kind, provider, model, time, person. Its cost is counts times the [price table](/account/price-table):

| | |
| --- | --- |
| Cost of a message | input tokens × input price + output tokens × output price + cache read tokens × cache read price + cache write tokens × cache write price, with the prices valid on the message's day |

A message without counts has no cost and shows as a count error, never as zero. A model missing from the price table is labelled unknown, never zero. An agent's own cost figure is kept as a fact and is never the figure shown.

## Tags say what, attributes say who {#tags}

A [split](/attribute/rules#splits) puts fractions of a session on tag sets, such as `feature: KD-1`. Tag values are records with default tags: a feature brings its project, a project brings its client. People carry [attributes](/account/attributes) with defaults of their own: team web brings department engineering. So a split's full tag set is:

| | |
| --- | --- |
| Full tag set of a split | its own tags, plus every default tag they bring, plus the person's attributes and their defaults |

A split with a feature is **direct**, with a project and no feature **project overhead**, with neither **organization overhead**. It always has its person.

## One formula for every report {#formula}

| | |
| --- | --- |
| Cost of a filter over a date range | the sum, over every split whose full tag set contains the filter and whose message falls in the range, of fraction × message cost |

A filter is any set of tags and attributes: a feature, a client, a team, `team: web` and `project: shop` together. For one tag name, the values partition the total, so cost per project adds up to the whole. Two filters on different names overlap (`team: web` and `project: shop` can count the same money) and are never added up. See [Reports](/report/reports).

## The bill {#bill}

| | |
| --- | --- |
| Bill of a provider over its date range | cost of that provider's messages in the range + what the bill has that no message explains |

The second part is a line on the bill, never negative: usage in the provider's own chat app, per-call fees the agent does not report. Negative means a message is wrong. See [Bills](/account/bills).

## A worked month {#month}

One provider bills 1,000 for 1 to 31 March. Features KD-1 and KD-2 have default project Shop; Shop has default client Acme. Mara and Tom have attribute team web; team web has default department engineering. Tom holds the service token of the review bot.

| Session | Person | Cost | Split | Deepest tag | Label |
| --- | --- | ---: | --- | --- | --- |
| S1 | Mara | 400 | 100 % | KD-1 | direct |
| S2 | Mara | 300 | 60 % = 180 | KD-1 | direct |
| | | | 40 % = 120 | KD-2 | direct |
| S3 | Tom (review bot) | 100 | 100 % | Shop | project overhead |
| S4 | Tom | 150 | 100 % | none | organization overhead |
| All messages | | 950 | | | |
| What no message explains | | 50 | | chat app usage | |
| Bill | | 1,000 | | | |

Check: 400 + 300 + 100 + 150 + 50 − 1,000 = 0.

Every slice of the same 950: person Mara 700, Tom 250. Feature KD-1 580, KD-2 120, Shop only 100, none 150. Label direct 700, project overhead 100, organization overhead 150. Project Shop 800, none 150. Client Acme 800. Team web 950. Department engineering 950.

## Plans and deviation {#plans}

An [estimate](/account/estimates) is a distribution of what described work will cost. Accepting one makes it the plan for its tag set.

| | |
| --- | --- |
| Plan of a filter over a date range | the accepted estimate's amount, spread over the range by the days of the estimate's own date range |
| Plan of a person | the sum of their shares: fraction × plan of that project over the days the share covers |
| Deviation | plan − cost, for any filter and range. Positive is favourable. |

In the worked month: the accepted estimate for Shop is 750 over 100 days, so March (31 days) gets 232.50; cost is 800, so the deviation is −567.50, unfavourable. KD-1 was accepted at a median of 500 and cost 580: −80, labelled "scope". Mara holds a 0.6 share of Shop from 1 March, so her plan for March is 0.6 × 232.50 = 139.50. See [Deviation and forecast](/report/deviation).

## Agents {#agents}

An agent is software that calls a model on a person's behalf. Token Controller keeps no record of agents, only their messages, each with its agent harness, model and optional agent name. A person typing in a provider's own chat app is not an agent; that usage is what the bill has that no message explains. Transcripts and prompts never enter; see [Security and privacy](/security).

## Local first {#local}

The [CLI](/collect/cli) is fully useful with no service connected, including an export for a spreadsheet. The local tool and the service talk over the documented [API](/reference/api), so the local tool can connect to any compatible service, or none.
