---
title: "Token entries endpoint"
description: "Report token entries from any agent with one HTTP call per session."
---

Any agent that can make an HTTP call can report its own token entries. Send one per session.

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

The service token decides the agent manager: the person who created it. Reported cost is optional; without it, Token Controller computes the list value from the model and the price table. Every field is described in the [token entry schema](/reference/token-entry).

## Tags {#placing}

Name a ticket or a project in the call and add any tags on your organization's list, or let your rules set them. Tags named in the call win over every rule. When nothing places it, it is organization overhead with its person. See the [order](/attribute/rules#order).

## Automatic {#automatic}

Token entries from the endpoint arrive automatically, with the status submitted. They never go on a token sheet.
