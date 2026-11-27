---
title: "Price tables"
description: "Rows of provider, model, token kind, price per million and valid from; which row counts, the managed table from LiteLLM's price list, an organization's rows, a contract's own prices, the file for self-hosting, and when a message is not priced yet."
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
| The managed table | Built into every edition: [LiteLLM's price list](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json) (MIT licence), with our own rows on top. Managed rows cannot be reversed. |
| A file | A self-hosted instance may read a CSV with the first five columns above; see [Configuration](/self-hosting/configuration#price-table). Its rows count as managed rows and replace a built-in row with the same provider, model, kind and valid-from date. |
| The organization | Rows an org manager adds, for a model the managed table lacks or a price you negotiated. Each is an event in the log: never edited, and a wrong one is reversed. |
| A contract | An organization's row with a `contract`, for a contract with prices of its own. |

### The managed table {#litellm}

The managed table is two files in the [public repository](https://github.com/tokencontroller/tokencontroller/tree/main/prices), built into the service:

- `prices/litellm.csv`: LiteLLM's prices, read from one LiteLLM commit (named in `prices/litellm-commit`) and refreshed every week. A price LiteLLM changes becomes a new row valid from the day we read it, so earlier figures keep the earlier price. A model LiteLLM drops keeps its rows. A model seen for the first time is valid from 2025-01-01.
- `prices/overrides.csv`: our own rows. For its provider, model and kind, our row replaces every LiteLLM row. If LiteLLM ever stops keeping its list, this file carries the table on its own.

Only text models (LiteLLM's modes `chat`, `responses` and `completion`) with an input and an output price are taken, under these providers. A LiteLLM provider not in this table is left out; the provider is never guessed.

| LiteLLM provider | Provider here |
| --- | --- |
| `anthropic` | `anthropic` |
| `openai` | `openai` |
| `gemini` | `google` |
| `vertex_ai`, `vertex_ai-anthropic_models`, `vertex_ai-language-models`, `vertex_ai-mistral_models` | `gcp_vertex` |
| `bedrock`, `bedrock_converse` | `aws_bedrock` |
| `azure`, `azure_ai` | `azure_foundry` |
| `github_copilot` | `github` |

The model is LiteLLM's name without its provider prefix, such as `claude-sonnet-4-5@20250929` for `vertex_ai/claude-sonnet-4-5@20250929`. Names that still hold a `/` after that (regional or fine-tuned entries) are left out, and so is a name two LiteLLM entries price differently.

| LiteLLM field | Token kind |
| --- | --- |
| `input_cost_per_token` | `input` |
| `output_cost_per_token` | `output` |
| `cache_read_input_token_cost` | `cache_read` |
| `cache_creation_input_token_cost` | `cache_write`; where LiteLLM has none, the input price, as those providers bill cache writes as input |
| `cache_creation_input_token_cost_above_1hr` | `cache_write_1h` |

LiteLLM gives prices per token; the table holds them per million tokens. A price of zero is left out, so a model LiteLLM lists without a real price stays not priced yet instead of costing nothing.

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
