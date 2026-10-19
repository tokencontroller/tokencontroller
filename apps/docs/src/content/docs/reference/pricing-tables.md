---
title: "Price tables"
description: "Rows of provider, model, token kind, price per million and valid from; the managed table, overrides, the file for self-hosting, and the rule for missing prices."
---

The price table is the only cost basis: cost is token counts by kind times the row valid on the message's day. [Price table](/account/price-table) says how to set it up; this page is the lookup.

## Rows {#rows}

| Column | Meaning |
| --- | --- |
| `provider` | `anthropic`, `openai`, `google`, `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or `other`. |
| `model` | The model name as the agent harness reports it. |
| `kind` | `input`, `output`, `cache_read` or `cache_write`. |
| `price_per_million` | US dollars per million tokens, as a decimal string. |
| `valid_from` | The first day the price applies. |

A row is an event. The row that counts for a message is the latest `valid_from` on or before the message's day, as of the report's date. Rows are never edited; a wrong row is reversed.

## Where rows come from {#source}

| Source | Rows |
| --- | --- |
| The managed table | Our list, following the providers' published prices, pulled in as events once a day on the hosted service and with an Enterprise key. |
| The organization | Rows an org manager adds. A row with the same provider, model, kind and `valid_from` as a managed row overrides it; the override wins. |
| A file | A self-hosted open edition ships without prices and reads a CSV with the five columns above; see [Configuration](/self-hosting/configuration#price-table). |

## Without a price {#unknown}

| | |
| --- | --- |
| A message with no counts | A count error: no cost, listed on its own, never zero. |
| A model with no row on the message's day | Unknown model: listed with its counts and no cost, never zero. Reconciliation shows it as cost the bill has that no message explains. |

## File format {#file}

```csv
provider,model,kind,price_per_million,valid_from
anthropic,claude-opus-5,input,15.00,2026-01-01
anthropic,claude-opus-5,output,75.00,2026-01-01
anthropic,claude-opus-5,cache_read,1.50,2026-01-01
anthropic,claude-opus-5,cache_write,18.75,2026-01-01
```
