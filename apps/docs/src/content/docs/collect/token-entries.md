---
title: "Token entries endpoint"
description: "Report token entries from any agent with one HTTP call per session."
---

Any agent that can make an HTTP call can report its own token entries. Send one per session.

![Token entries: every session with its agent, model, tickets and tags](/screens/11-token-entries.png)

## The call {#call}

```http
POST https://app.tokencontroller.com/api/v1/token-entries
Authorization: Bearer tc_svc_…

{
  "agent_harness": "support-desk",
  "agent_name": "email answers",
  "model": "claude-sonnet-5",
  "started_at": "2026-09-18T09:12:00Z",
  "ended_at": "2026-09-18T09:14:10Z",
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200 },
  "reported_cost": 0.031,
  "summary": "Answered refund question for order 1182",
  "project": "support",
  "tags": { "client": "acme", "purpose": "support" }
}
```

The person who created the service token is the agent manager, and the token entries are [automatic](/collect#automatic). Reported cost is optional; without it, Token Controller computes the list value from the model and the [price table](/reference/pricing-tables). Every field is in the [token entry schema](/reference/token-entry).

To send several token entries in one call, send a list of up to 1,000. Either all of them are saved or none.

## The answer {#answer}

`201 Created` with what was saved, one object per token entry, or a list for a list:

```json
{
  "id": "0b6f6c1e-5d1c-4c62-9d0a-3f3a3c9f1a52",
  "status": "submitted",
  "list_value": "0.03",
  "currency": "USD",
  "cost_kind": "project_overhead",
  "new_tags": []
}
```

`list_value` is `null` for an unknown model until the price table knows it. Token Controller accepts every token entry and fits the pieces together afterwards:

- A tag that is not on your organization's list is kept, and `new_tags` names it. The org manager adds it to the list or maps it to an existing one.
- A token entry for a session Token Controller already has is a [duplicate](/glossary#duplicate): accepted, counted once, and shown greyed out behind a toggle.
- A token entry for a closed period is accepted and waits for the org manager to post it as an [adjustment](/account/close#adjustments).

Only a request that is not a token entry at all, such as broken JSON, gets `400`.

## Tags {#placing}

Name a ticket or a project in the call and add any tags on your organization's list, or let your rules set them. Tags named in the call win over every rule. When nothing places it, it is organization overhead with its person. See the [order](/attribute/rules#order).
