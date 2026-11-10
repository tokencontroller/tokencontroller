---
title: "Bills and reconciliation"
description: "Connect Anthropic, OpenAI or a CSV bill, and the check on each bill's own range: bill equals the cost of its messages plus what no message explains."
---

When you use AI agents, you may get bills from more than one provider, or two bills from the same one, such as an API account and a seat plan. A bill is what a provider charged you under one contract over a date range: contract, first and last day, amount, currency. A metered contract, such as an API account, bills per use. A flat contract, such as a seat plan, bills its fee plus any extra usage. This page shows how to bring bills in and check each one against what your agents used.

## What is reconciled {#what}

| Provider | How to connect | The bill is | Figures are final |
| --- | --- | --- | --- |
| [Anthropic, Enterprise plan](#anthropic-enterprise), usage at API rates | Enterprise Analytics API key | The cost report at list price | About 30 days after the month ends |
| [Anthropic Console](#anthropic-console) or API keys | Console Admin API key | The Usage and Cost report, per workspace and day | When the month ends |
| [OpenAI](#openai) API | OpenAI Admin key | The Costs API, per day, project and API key | When the month ends |
| Bedrock, Vertex, Foundry | The cloud's export as [CSV](#csv), or a connector on an Enterprise plan contract | The cloud's export | On upload |
| Anything else | [CSV](#csv), typed in by hand, or the [bills endpoint](/reference/api/bills#send) | That bill | On upload |
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

Any bill can be uploaded as CSV, and this is the way for Bedrock, Vertex and Foundry. Under _Bills_, choose _Upload CSV_ and give the provider, the contract, first and last day, amount and currency (see the [bill fields](/reference/bill)), or upload the cloud's own cost export. The upload shows what it read before anything is saved: each bill in the file, and whether it is new, restates the bill in force, or has the same figures. A file with a problem names the row and column and saves nothing.

![The CSV upload: what the file holds, each bill against the bill in force, before anything is saved](/screens/17-upload-csv-bill.png)

## Restated bills and currency {#sources}

A provider can restate a bill. The new figure is a new bill event for the same contract and range; the old one is reversed and stays visible. A pull with the same figures changes nothing. A bill in another currency keeps its own amount and the exchange rate of its last day.

## Reconciliation {#reconciliation}

Each bill has one contract, and the check runs on the bill's own date range against the messages with the same contract:

| Contract | Check |
| --- | --- |
| Metered | Bill = cost of the messages with this contract in the range + what the bill has that no message explains |
| Flat | Bill = fee + extra usage. The cost of its messages minus extra usage is covered by the plan. |

What no message explains is a line on the bill: usage in the provider's own chat app, or per-call fees the agent does not report. It must be zero or positive. Negative means a message is wrong: a wrong count, a wrong model, a price row that is too high, or a message under the wrong contract. Find it and reverse it.

Beside every check you see the messages of the same provider that have no contract yet, and those whose contract came only from the provider having one contract. How a message gets its contract is on the [data model](/reference/data-model#contract) page, and the [worked month](/concepts#month) checks three bills.

![Bills and the reconciliation: bill, cost of messages, what no message explains, per provider](/screens/06-bills-and-reconciliation.png)

## As of {#as-of}

A bill is an event like every other. A reconciliation as of the [cutoff](/account/cutoff) uses the bills and messages known then; a restated bill after the cutoff shows as of today and waits for an org manager to adjust it in.
