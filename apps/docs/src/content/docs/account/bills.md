---
title: "Bills"
description: "Where bills come from, reconciliation, and list value against billed value."
---

A bill is a provider's figure for one period's metered usage: gross, discount, credit and net. Metered usage is everything priced per use: tokens, and per-call fees such as web search. Token entries must add up to it. Seats, subscriptions and other flat fees are not usage: they stay off the bill and off the cost sheet, and finance handles them as before.

## Where bills come from {#sources}

| Provider | How |
| --- | --- |
| Anthropic, Enterprise plan | [Enterprise Analytics API](/integrations/anthropic-enterprise) |
| Anthropic Console | [Usage and Cost Admin API](/integrations/anthropic-console) |
| OpenAI | [Costs API](/integrations/openai) |
| Bedrock, Vertex, Foundry | The cloud's export as [CSV](/integrations/csv), or a connector on an Enterprise plan contract |
| Anything else | CSV upload, typed in by hand, or the bills endpoint of the [HTTP API](/reference/api) |

A connected provider is pulled once a day, for the current and the previous period; _Pull now_ pulls at once. A provider can restate a bill. Each bill keeps its revisions, and a pull or upload with the same figures changes nothing. A bill in another currency than your reporting currency keeps its original amount and the exchange rate of its date.

## Reconciliation {#reconciliation}

For each period and each bill: token entries plus residual equal the bill. Every token entry has a person, so everything except residual is attributed. Tags play no part in this check. With several providers, each bill is checked on its own and the cost sheet adds them up to the organization total. Residual is the part of the bill no token entry explains: usage in the provider's own chat app, per-call fees the agent harness does not report, a restated bill. It is always shown.

## What the close reconciles against {#what}

| Cost source | Reconciles against | List value is |
| --- | --- | --- |
| Enterprise plan, usage at API rates | The Enterprise Analytics cost report at list price; the discount is its own line | Money, before discount |
| Console or API key | The Usage and Cost report, per workspace and day | Money |
| Seat-based plans (Claude Team, Max, Pro) | Nothing: the seat is a flat fee and usage inside it is not metered in dollars. There is no bill to reconcile. | Tokens at API rates, labelled as an estimate on every figure. It answers what the usage would cost on an API plan. Every report can switch to the share view: each line's share of the period instead of dollars. |
| Bedrock, Vertex, Foundry | The cloud's export | Approximate |

The web app says which applies to your organization.

## List value and billed value {#values}

Every token entry has a list value: the agent's reported cost where it has one (Claude Code does), otherwise its token counts times the [price table](/reference/pricing-tables) for the model. A billed value exists where a bill or billed telemetry says what was actually charged. Every figure is labelled billed, list-price estimate, or unknown model.
