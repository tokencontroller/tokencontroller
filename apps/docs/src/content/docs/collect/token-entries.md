---
title: "Token entries endpoint"
description: "Report messages from any agent with one HTTP call per session. A token entry is one message or one session record."
---

Any agent that can make an HTTP call can report its own usage. A token entry is one message, or one session with its messages. Send one per session.

## The call {#call}

```http
POST https://app.tokencontroller.com/api/v1/token-entries
Authorization: Bearer tc_svc_…

{
  "agent_harness": "support-desk",
  "agent_name": "email answers",
  "provider": "anthropic",
  "model": "claude-sonnet-5",
  "started_at": "2026-09-18T09:12:00Z",
  "ended_at": "2026-09-18T09:14:10Z",
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200, "cache_write": 0 },
  "reported_cost": 0.031,
  "summary": "Answered refund question for order 1182",
  "project": "support",
  "tags": { "client": "acme", "purpose": "support" }
}
```

The person who created the service token is the person of the message. `tokens` with all four kinds is required; a token entry without counts is saved as a count error with no cost. `reported_cost` is optional and is kept as a fact, never shown as the cost: the cost comes from the [price table](/account/price-table). Every field is in the [token entry schema](/reference/token-entry); the endpoint itself is on the [API page](/reference/api/messages#token-entries).

To send several token entries in one call, send a list of up to 1,000. Either all of them are saved or none.

## The answer {#answer}

`201 Created` with what was saved, one object per token entry, or a list for a list:

```json
{
  "session_id": "support-desk:1182",
  "messages": 1,
  "cost": "0.03",
  "currency": "USD",
  "label": "project_overhead",
  "unknown_tags": []
}
```

`cost` is `null` with the reason `unknown_model` until the price table has the model, or `count_error` when counts are missing. Token Controller accepts every token entry and fits the pieces together afterwards:

- A tag key that names no tag value yet is kept on the message, and `unknown_tags` names it. Someone with assign creates the value or reverses the tag.
- A token entry for a message Token Controller already has, by session id and message id, is the same record, counted once.
- A token entry dated before the [cutoff](/account/cutoff) is accepted, shown, and waits for an org manager to adjust it in.

Only a request that is not a token entry at all, such as broken JSON, gets `400`.

## Tags {#placing}

Name a feature or a project in the call and add any tags, or let your rules set them from the signals you send. Tags named in the call win over every rule. When nothing places it, it is organization overhead with its person. See the [order](/attribute/rules#order).
