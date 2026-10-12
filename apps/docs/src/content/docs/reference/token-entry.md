---
title: "Token entry schema"
description: "The fields of a token entry."
---

The fields of a token entry, as the token entries endpoint and CSV import take them.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `agent_harness` | The product the agent ran on. Required. |
| `agent_name` | A label for the agent, such as "review bot". Optional. |
| `model` | The model that did the work. Required. |
| `provider` | Who charges for the model calls: `anthropic`, `openai`, `google`, `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or `other`. Optional; without it, it follows from the model's name. Bills are matched per provider. |
| `effort`, `speed` | The effort and speed settings the agent ran with, such as `high` and `fast`. Optional. The [agent harness report](/report/agent-harness) shows cost by them. |
| `started_at`, `ended_at` | When the session started and ended. Required. |
| `tokens` | Counts by kind: `input`, `output`, `cache_write`, `cache_read`. Required. |
| `reported_cost` | The agent's own cost figure in US dollars, as a number or a decimal string such as `"0.031"`. Optional; without it the list value is computed. |
| `summary` | What the session did, from one line to a short report. With the tag values, the only text. |
| `ticket`, `project` | Where it belongs. Optional. A ticket named here wins over every rule; without one, [rules place it](/attribute/rules#order). |
| `tags` | Any tags from the organization's list, such as `{"team": "web", "purpose": "code-review"}`. One value per tag name. Optional. Tags sent here win over inherited ones. |
| `signals` | Branch, working directory, repository, pull request number, subagent type, and ticket keys found in prompts, the session title and the pull request title. Optional. |
| `session_id` | Joins the token entry to telemetry. Optional. Sending a token entry again with the same session ID updates it. |
| `messages` | The session's model calls, each with `message_id`, `at`, `tokens` and optionally `request_id`, `model` and `reported_cost`. Optional. With them, a session that telemetry also reported is counted exactly once, call by call. Their tokens must add up to `tokens`. |
| `splits` | How the token entry is split, each split with its own `tags` and `signals`: by `message_ids` for segments, which is exact, or by `share_percent` by hand, adding up to 100. Optional; without it the token entry is one split. |
| `local_id` | The sender's own ID for the token entry, such as `claude-code:<session id>`. Optional. Sending it again replaces the earlier one. |
| `placed_by` | What placed it: a rule's name, `hand`, `classifier` or `none`. Optional. |

Set by Token Controller: agent manager, list value, billed value, inherited tags, splits, status, and candidate tickets: other tickets the session named that no split is placed on.
