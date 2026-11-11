---
title: "Price table"
description: "The price table is the only cost basis: the managed table on the hosted service, your own rows for every contract or for one, your own file when self-hosted, and what happens when a message cannot be priced yet."
---

Your providers may charge different prices for the same model, and you may have negotiated prices of your own. The price table is the only thing that turns token counts into cost, so every report shows cost at the same prices. A message the table cannot price for certain is shown as not priced yet, with its token counts, never as zero.

## Rows {#rows}

| Column | Example |
| --- | --- |
| Provider | `anthropic` |
| Model | `claude-opus-5` |
| Token kind | `input`, `output`, `cache_read`, `cache_write`, `cache_write_5m`, `cache_write_1h`, or any other kind a source sends |
| Price per million tokens | `15.00`, in US dollars |
| Valid from | `2026-09-01` |
| Contract | Optional. A row for one contract wins over the rows for every contract. |

A message's cost uses the rows valid on the message's day, kind by kind. A row is a record: a new price is a new row with a later valid-from date, and figures as of an earlier date keep the earlier price. Only kinds with a row are summed; a kind with no row is listed on the message and the rest of it is still priced. `cache_write` is a cache write of unknown duration and is priced as 5 minutes. See [token kinds](/reference/data-model#token-kinds) and the [reference](/reference/pricing-tables).

## Where the table comes from {#source}

| Edition | Price table |
| --- | --- |
| Hosted (Solo, Team plan) | The managed table, pulled in as events from our list, which follows the providers' published prices. Your organization adds rows and overrides rows; your row wins. |
| Enterprise plan | The managed table, hosted by us or on your own servers. The image carries the table as of its build, and with outbound HTTPS to `app.tokencontroller.com` the instance fetches newer prices with its license key every six hours. An instance without internet access gets newer prices with the next image, or as a file from us. |
| Open edition, self-hosted | Ships without prices. Bring your own file; see [Configuration](/self-hosting/configuration#price-table). |

## Your own rows {#overrides}

Under _Price table_, an org manager chooses _Add a row_ for a model the managed table lacks, or for a price you negotiated: the same provider, model and token kind with a valid-from date. Your row wins over the managed row from that date. Leave _Contract_ empty for every contract, or pick one when only that contract has the price, such as a discount on one API account. A wrong row is reversed, never edited.

## When a message cannot be priced yet {#missing}

| | Shown as |
| --- | --- |
| A message without token counts | A count error. It has no cost and is listed on its own. |
| A model with no row on the message's day | Not priced yet. |
| A message with no provider, and the providers in the table charge different prices for its model | Not priced yet. With no provider, a message is priced only when every provider in the table charges the same for that model. |

The provider is never guessed from the model's name. A model name such as `anthropic/claude-sonnet-4-5` that arrives without a provider stays not priced yet until a provider is sent or the message gets a contract, whose provider then counts.

A message not priced yet keeps its token counts and shows on its own line in every report. The _Price table_ screen lists every message without a cost, and the [check](/account/bills#reconciliation) of a bill lists them too: their cost is part of what no message explains until the counts or the row arrive.
