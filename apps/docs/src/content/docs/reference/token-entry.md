---
title: "Token entry schema"
description: "The fields of a token entry, as the token entries endpoint and CSV import take them."
---

The fields of a token entry, as the [token entries endpoint](/collect/token-entries) takes them, one entry or a list. [CSV import](/collect/manual#csv) takes the same fields as columns.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `agent_harness` | The product the agent ran on, such as `claude-code` or `support-desk`. Required. |
| `agent_name` | A label for the agent, such as "review bot". Optional. |
| `model` | The model that did the work. Required. |
| `provider` | Who charges for the model calls: `anthropic`, `openai`, `google`, `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or `other`. Optional; without it, it follows from the model's name. Bills are matched per provider. |
| `effort`, `speed` | The effort and speed settings the agent ran with, such as `high` and `fast`. Optional. The [agent harness report](/report/reports#agent-harness) shows cost by them. |
| `started_at`, `ended_at` | When the session started and ended. Required. |
| `tokens` | Counts by kind: `input`, `output`, `cache_write`, `cache_read`. Required. |
| `reported_cost` | The agent's own cost figure in US dollars, as a number or a decimal string such as `"0.031"`. Optional; without it the list value is computed from the model and the [price table](/reference/pricing-tables). |
| `summary` | What the session did, from one line to a short report. With the tag values, the only text stored. |
| `ticket`, `project` | Where it belongs: a ticket key such as `KD-214`, a project by name. Optional. A ticket named here wins over every rule; without one, [rules place it](/attribute/rules#order). |
| `tags` | Any tags from the organization's list, such as `{"team": "web", "purpose": "code-review"}`. One value per tag name. Optional. Tags sent here win over inherited ones. |
| `signals` | Facts about the session that hint at its tags. Optional. See [Signals](#signals). |
| `session_id` | Joins the token entry to telemetry. Optional. Sending a token entry again with the same session ID keeps both: the newer one counts, the older one is a [duplicate](/glossary#duplicate). |
| `messages` | The session's model calls. Optional. With them, a session that telemetry also reported is counted exactly once, call by call. See [Messages](#messages). |
| `splits` | How the token entry is split. Optional; without it the token entry is one split. See [Splits](#splits). |
| `local_id` | The sender's own ID for the token entry, such as `claude-code:<session id>`. Optional. Sending it again replaces the earlier one. |
| `placed_by` | What placed it: a rule's name, `hand`, `classifier` or `none`. Optional. |

Set by Token Controller: agent manager, list value, billed value, inherited tags, splits, status, and candidate tickets: other tickets the session named that no split is placed on.

## Signals {#signals}

| Signal | Meaning |
| --- | --- |
| `branch` | The git branch. |
| `working_directory` | The working directory. |
| `repository` | The repository, such as `github.com/kestrel/storefront`. |
| `pull_request` | The pull request number. |
| `subagent_type` | The subagent type. |
| `ticket_keys` | Ticket keys found in prompts, the session title and the pull request title. Only the keys, never the text. |

## Messages {#messages}

Each message is one model call. Their tokens must add up to the entry's `tokens`.

| Field | Meaning |
| --- | --- |
| `message_id` | The agent harness's ID for the call, unique within the session. Required. |
| `at` | When the call was made. Required. |
| `tokens` | Counts by kind, as above. Required. |
| `request_id` | The provider's request ID, if the agent harness knows it. Optional. |
| `model` | The model of this call, when it differs from the token entry's. Optional. |
| `reported_cost` | The agent's own cost figure for this call, in US dollars. Optional. |

## Splits {#splits}

Each split has its own `tags` and `signals`, and either of:

| Field | Meaning |
| --- | --- |
| `message_ids` | The messages this segment holds. Exact. |
| `share_percent` | A share set by hand, in percent. The shares add up to 100. |
