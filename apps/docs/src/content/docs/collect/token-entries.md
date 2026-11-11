---
title: "Token entries endpoint"
description: "Report messages from any agent with one HTTP call per session. A token entry is one message or one session record."
---

You may run an agent of your own, such as a support bot, that no collector of ours knows. Any agent that can make an HTTP call can report its own usage. A token entry is one message, or one session with its messages. Send one per session.

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
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200 },
  "reported_cost": "0.031",
  "summary": "Answered refund question for order 1182",
  "project": "support",
  "tags": { "client": "acme", "purpose": "support" },
  "signals": { "queue": "refunds" }
}
```

The person who created the service token is the person of the message. `tokens` holds the counts by kind; a kind left out has no tokens, and any other kind, such as `cache_write_1h`, is sent by its name. A token entry with no kind at all is a count error with no cost.

Send `provider` when you know who charges for the calls, spelled as bills spell it, such as `anthropic`, `aws_bedrock` or `openai`. It is never guessed from the model's name. Without it, the message is priced when every provider in the price table charges the same for its model, and is otherwise [not priced yet](/account/price-table#missing) until it gets a contract.

`signals` takes anything else that may help to place the cost, by any name. A signal named like one of your tag names, such as `project`, is your own assignment and wins over rules. `reported_cost` is optional and is kept as a signal, never shown as the cost: the cost comes from the [price table](/account/price-table). Every field is in the [token entry schema](/reference/token-entry); the endpoint itself is on the [API page](/reference/api/messages#token-entries).

To send several token entries in one call, send a list of up to 1,000. Either all of them are saved or none. On the hosted service, a list that would pass the organization's [messages per day](/plans#messages) is refused with `429`.

## What is kept {#received}

The endpoint stores the body it received as a received record before it reads it. A signal or session fact that holds content text, such as one named `prompt` or `response`, is removed first unless your organization switched keeping it on. The summary is kept. See [What the usage endpoints keep](/collect#received).

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

`cost` is the session's cost from the price table as of now. A message not priced yet, or a count error, adds nothing to it and shows on its own line in reports. Token Controller accepts every token entry and fits the pieces together afterwards:

- A tag key that names no tag value yet is kept on the message, and `unknown_tags` names it. Someone with assign creates the value or reverses the tag.
- A token entry for a message Token Controller already has, by session id and message id, is the same record, counted once.
- A token entry dated before the [cutoff](/account/cutoff) is accepted, shown, and waits for an org manager to adjust it in.

Only a request that is not a token entry at all, such as broken JSON or a field the schema does not have, gets `400`, and no message is made from it.

## Tags {#placing}

Name a feature or a project in the call and add any tags, or let your rules set them from the signals you send. Tags named in the call win over every rule. When nothing places it, it is organization overhead with its person. See the [order](/attribute/rules#order).
