---
title: "Price table"
description: "The price table is the only cost basis: the managed table on the hosted service, your own rows and overrides, your own file when self-hosted, and what happens without a price."
---

The price table is the only thing that turns token counts into cost. It is required: a message of a model with no price has a cost labelled unknown model, never zero.

## Rows {#rows}

| Column | Example |
| --- | --- |
| Provider | `anthropic` |
| Model | `claude-opus-5` |
| Token kind | `input`, `output`, `cache_read` or `cache_write` |
| Price per million tokens | `15.00`, in US dollars |
| Valid from | `2026-09-01` |

A message's cost uses the row valid on the message's day. A row is a record: a new price is a new row with a later valid-from date, and figures as of an earlier date keep the earlier price. See the [reference](/reference/pricing-tables).

## Where the table comes from {#source}

| Edition | Price table |
| --- | --- |
| Hosted (Solo, Team plan) | The managed table, pulled in as events from our list, which follows the providers' published prices. Your organization adds rows and overrides rows; the override wins. |
| Enterprise plan | The key includes the managed table, hosted by us or on your own servers. |
| Open edition, self-hosted | Ships without prices. Bring your own file; see [Configuration](/self-hosting/configuration#price-table). |

## Your own rows {#overrides}

Under _Settings_, _Price table_, an org manager adds a row for a model the managed table lacks, or a row with the same provider, model, token kind and a valid-from date for a price you negotiated. Your row wins over the managed row from that date. A wrong row is reversed, never edited.

## When a count or a price is missing {#missing}

| | Shown as |
| --- | --- |
| A message without token counts | A count error. It has no cost and is listed on its own. |
| A model with no row on the message's day | Unknown model. The message is listed with its counts and no cost, never zero. |

Both show in [reconciliation](/account/bills#reconciliation) as cost the bill has that no message explains, until the counts or the row arrive.
