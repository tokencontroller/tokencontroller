---
title: "Price tables"
description: "Rows of provider, model, token kind, price per million and valid from; which row counts, the managed table, an organization's rows, a contract's own prices, the file for self-hosting, and when a message is not priced yet."
---

The price table is the only cost basis: cost is token counts by kind times the row valid on the message's day. [Price table](/account/price-table) says how to set it up; this page is the lookup.

## Rows {#rows}

| Column | Meaning |
| --- | --- |
| `provider` | Who charges: `anthropic`, `openai`, `google`, `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or `other`. |
| `model` | The model name as the agent harness reports it, without a provider in front. |
| `kind` | A [token kind](/reference/data-model#token-kinds), such as `input`, `output`, `cache_read`, `cache_write`, `cache_write_5m` or `cache_write_1h`. |
| `price_per_million` | US dollars per million tokens, as a decimal string. |
| `valid_from` | The first day the price applies. |
| `contract` | Optional: the contract whose own price this is. The row counts only for that contract's messages, and its provider must be the contract's. |

## Which row counts {#which}

For each kind of a message, among the rows for its provider and model that are valid on its day, as of the report's date:

1. The row of the message's contract, if it has one.
2. Else the organization's own row.
3. Else the managed row.

Among rows of the same step, the latest `valid_from` wins. A `cache_write` count with no row of its own takes the `cache_write_5m` price, and the other way round.

The provider is the contract's provider when the message has a contract, else the one the sender reported. It is never guessed from the model's name. A message with no provider is priced when every provider in the table charges the same for its model.

## Where rows come from {#source}

| Source | Rows |
| --- | --- |
| The managed table | Our list, following the providers' published prices, on the hosted service and with an Enterprise key. Managed rows cannot be reversed. |
| A file | A self-hosted open edition ships without prices and reads a CSV with the first five columns above; see [Configuration](/self-hosting/configuration#price-table). Its rows count as managed rows. |
| The organization | Rows an org manager adds, for a model the managed table lacks or a price you negotiated. Each is an event in the log: never edited, and a wrong one is reversed. |
| A contract | An organization's row with a `contract`, for a contract with prices of its own. |

## Not priced yet {#unknown}

| | |
| --- | --- |
| A message with no counts | A count error: no cost, listed on its own, never zero. |
| No row for the model under the message's provider on its day | Not priced yet: listed with its token counts and no cost, never zero. |
| No provider, and the providers in the table charge differently for the model, or none has a row | Not priced yet. Sending the provider, or putting the message under a contract, prices it. |
| A kind with tokens and no row | Listed on the message and left out of the sum. The rest of the message is priced. |

Every report shows messages not priced yet on a line of their own with their token counts. In the [check](/account/bills#reconciliation), their cost is part of what no message explains until a row is added.

## File format {#file}

```csv
provider,model,kind,price_per_million,valid_from
anthropic,claude-opus-5,input,15.00,2026-01-01
anthropic,claude-opus-5,output,75.00,2026-01-01
anthropic,claude-opus-5,cache_read,1.50,2026-01-01
anthropic,claude-opus-5,cache_write,18.75,2026-01-01
```
