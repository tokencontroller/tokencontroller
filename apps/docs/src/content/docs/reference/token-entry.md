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
| `started_at`, `ended_at` | When the session started and ended. Required. |
| `tokens` | Counts by kind: `input`, `output`, `cache_write`, `cache_read`. Required. |
| `reported_cost` | The agent's own cost figure. Optional; without it the list value is computed. |
| `summary` | What the session did, from one line to a short report. With the tag values, the only text. |
| `ticket`, `project` | Where it belongs. Optional. A ticket named here wins over every rule; without one, [rules place it](/attribute/rules#order). |
| `tags` | Any tags from the organization's list, such as `{"team": "web", "purpose": "code-review"}`. One value per tag name. Optional. Tags sent here win over inherited ones. |
| `signals` | Branch, working directory, repository, pull request number, subagent type, and ticket keys found in prompts, the session title and the pull request title. Optional. |
| `session_id` | Joins the token entry to telemetry. Optional. |

Set by Token Controller: agent manager, list value, billed value, inherited tags, splits, status.
