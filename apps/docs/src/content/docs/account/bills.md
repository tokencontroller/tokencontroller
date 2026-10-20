---
title: "Bills and reconciliation"
description: "Connect Anthropic, OpenAI or a CSV bill, and the check on each bill's own range: bill equals the cost of its messages plus what no message explains."
---

A bill is a provider's figure for metered usage over a date range: provider, first and last day, amount, currency. Seats, subscriptions and other flat fees are not usage: they are not on a bill, and finance handles them as before.

## What is reconciled {#what}

| Provider | How to connect | The bill is | Figures are final |
| --- | --- | --- | --- |
| [Anthropic, Enterprise plan](#anthropic-enterprise), usage at API rates | Enterprise Analytics API key | The cost report at list price | About 30 days after the month ends |
| [Anthropic Console](#anthropic-console) or API keys | Console Admin API key | The Usage and Cost report, per workspace and day | When the month ends |
| [OpenAI](#openai) API | OpenAI Admin key | The Costs API, per day, project and API key | When the month ends |
| Bedrock, Vertex, Foundry | The cloud's export as [CSV](#csv), or a connector on an Enterprise plan contract | The cloud's export | On upload |
| Anything else | [CSV](#csv), typed in by hand, or the bills endpoint of the [HTTP API](/reference/api) | That bill | On upload |
| Seat plans (Claude Team, Max, Pro) | Nothing to connect | No bill: the seat is a flat fee. Cost is still computed from the price table. | |

## Connecting a provider {#connect}

All keys are added under _Bills_. A connected provider is pulled once a day for the last 62 days; _Pull now_ pulls at once.

### Anthropic Enterprise {#anthropic-enterprise}

For organizations on Anthropic's Enterprise plan, billed for usage at API rates. The organization's primary owner creates an Enterprise Analytics API key in claude.ai under _Organization settings_, _API_, with the `read:analytics` scope. Anthropic's figures become final about 30 days after the month ends; until then the bill is marked not final.

### Anthropic Console {#anthropic-console}

For organizations that pay Anthropic per token through the Console or API keys. An organization admin creates an Admin API key (it starts with `sk-ant-admin`) in the Claude Console. Token Controller reads the Usage and Cost report, per workspace and day.

### OpenAI {#openai}

For organizations that pay OpenAI for API usage. An organization owner creates an Admin key (it starts with `sk-admin-`) under _Organization settings_, _Admin keys_. Token Controller reads the Costs API.

### CSV {#csv}

Any bill can be uploaded as CSV, and this is the way for Bedrock, Vertex and Foundry. Under _Bills_, choose _Upload CSV_ and give the provider, first and last day, amount and currency (see the [bill fields](/reference/bill)), or upload the cloud's own cost export. The upload shows what it read before anything is saved: each bill in the file, and whether it is new, restates the bill in force, or has the same figures. A file with a problem names the row and column and saves nothing.

![The CSV upload: what the file holds, each bill against the bill in force, before anything is saved](/screens/17-upload-csv-bill.png)

## Restated bills and currency {#sources}

A provider can restate a bill. The new figure is a new bill event for the same provider and range; the old one is reversed and stays visible. A pull with the same figures changes nothing. A bill in another currency keeps its own amount and the exchange rate of its last day.

## Reconciliation {#reconciliation}

Reconciliation runs on the bill's own date range, per provider:

| | |
| --- | --- |
| Bill | cost of every message of that provider in the range + what the bill has that no message explains |

The second part is a line on the bill, never a concept of its own: usage in the provider's own chat app, per-call fees the agent harness does not report, a message whose model is missing from the price table. It must be zero or positive. Negative means a message is wrong: a wrong count, a wrong model, a price row that is too high, or a message counted that the provider did not charge. Find it and reverse it.

Tags play no part in the check. With several providers, each bill is checked on its own; the organization total is the sum of the bills. In the [worked month](/concepts#month), messages cost 950, the bill is 1,000, and 50 is chat app usage no message explains.

![Bills and the reconciliation: bill, cost of messages, what no message explains, per provider](/screens/06-bills-and-reconciliation.png)

## As of {#as-of}

A bill is an event like every other. A reconciliation as of the [cutoff](/account/cutoff) uses the bills and messages known then; a restated bill after the cutoff shows as of today and waits for an org manager to adjust it in.
