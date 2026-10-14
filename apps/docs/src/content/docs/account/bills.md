---
title: "Bills"
description: "Connect Anthropic, OpenAI or a CSV bill, what the close reconciles against, and list value against billed value."
---

A bill is a provider's figure for one period's metered usage, and the total your token entries must add up to. Seats, subscriptions and other flat fees are not usage: they stay off the bill and off the cost sheet, and finance handles them as before.

## What the close reconciles against {#what}

Your organization's [cost source](/account/organizations) decides which row applies.

| Provider | How to connect | The close reconciles against | Figures are final |
| --- | --- | --- | --- |
| [Anthropic, Enterprise plan](#anthropic-enterprise), usage at API rates | Enterprise Analytics API key | The cost report at list price; the discount is its own line | About 30 days after the period ends |
| [Anthropic Console](#anthropic-console) or API keys | Console Admin API key | The Usage and Cost report, per workspace and day | When the period ends |
| [OpenAI](#openai) API | OpenAI Admin key | The Costs API, per day, project and API key | When the period ends |
| Bedrock, Vertex, Foundry | The cloud's export as [CSV](#csv), or a connector on an Enterprise plan contract | The cloud's export | On upload |
| Anything else | [CSV](#csv), typed in by hand, or the bills endpoint of the [HTTP API](/reference/api) | That bill | On upload |
| Seat plans (Claude Team, Max, Pro) | Nothing to connect | Nothing: the seat is a flat fee, so there is no bill | No bill |

The close of a period waits until its bill is final.

## Connecting a provider {#connect}

All keys are added under _Bills_. A connected provider is pulled once a day, for the current and the previous period; _Pull now_ pulls at once.

### Anthropic Enterprise {#anthropic-enterprise}

For organizations on Anthropic's Enterprise plan, billed for usage at API rates. The organization's primary owner creates an Enterprise Analytics API key in claude.ai under _Organization settings_, _API_, with the `read:analytics` scope.

The close reconciles against the Enterprise Analytics cost report at list price, with the discount as its own line. Anthropic's figures become final about 30 days after the period ends, so the close of a period waits for them.

### Anthropic Console {#anthropic-console}

For organizations that pay Anthropic per token through the Console or API keys. An organization admin creates an Admin API key (it starts with `sk-ant-admin`) in the Claude Console. Token Controller reads the Usage and Cost report, per workspace and day.

### OpenAI {#openai}

For organizations that pay OpenAI for API usage. An organization owner creates an Admin key (it starts with `sk-admin-`) under _Organization settings_, _Admin keys_. Token Controller reads the Costs API.

### CSV {#csv}

Any bill can be uploaded as CSV, once per period, and this is the way for Bedrock, Vertex and Foundry. Under _Bills_, choose _Upload CSV_ and give the period, gross, discount, credit, net and currency (see the [bill fields](/reference/bill)), or upload the cloud's own cost export. The upload shows what it read before anything is saved. On an Enterprise plan contract, a connector can pull Bedrock, Vertex or Foundry bills instead.

## Revisions and currency {#sources}

A provider can restate a bill. Each bill keeps its revisions, and a pull or upload with the same figures changes nothing. A bill in another currency than your reporting currency keeps its original amount and the exchange rate of its date.

## Reconciliation {#reconciliation}

For each period and each bill, token entries plus residual equal the bill. With several providers, each bill is checked on its own, and the cost sheet adds them up to the organization total. Tags play no part in this check.

![Bills for a closed month and the reconciliation: bill, token entries, residual and attribution per provider](/screens/06-bills-and-reconciliation.png)

Every token entry has a person, so everything except residual is attributed. Residual is the part of the bill no token entry explains, such as usage in the provider's own chat app, per-call fees the agent harness does not report, or a restated bill. It is always shown.

## List value and billed value {#values}

Every token entry has a list value: the agent's reported cost where it has one (Claude Code does), otherwise its token counts times the [price table](/reference/pricing-tables) for the model. A billed value exists where a bill or billed telemetry says what was actually charged. Every figure is labelled billed, list-price estimate, or unknown model.

| Cost source | List value is |
| --- | --- |
| Enterprise plan, usage at API rates | Money, before discount |
| Console or API key | Money |
| Seat plans (Claude Team, Max, Pro) | Tokens at API rates, labelled as an estimate on every figure: what the usage would cost on an API plan. Every report can switch to the share view, each line's share of the period instead of money. |
| Bedrock, Vertex, Foundry | Approximate |
