---
title: "Organization and people"
description: "Settings, members and invites, grants, the cutoff, submissions and submit requests, API tokens, the price table with contract prices, and reversals."
---

One organization is one event log, one cutoff and one reporting currency. These endpoints are what an org manager and a people manager use to run it; most of them are made by a person in the web app, not by a token.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `GET /organization`, `PATCH /organization` | The settings; change them. | Anyone; org manager, in the web app |
| `GET /organization/member-limit` | The member count and the plan's limit. | Anyone |
| `GET /members` | Everyone in the organization. | Anyone |
| `POST /members` | Invite someone by email. | Org manager, in the web app |
| `PATCH /members/{id}`, `DELETE /members/{id}` | Change a member's name, email, role or filters; remove a member. | Org manager, in the web app |
| `PUT /members/{id}/filters` | The filters a people manager's role is granted on. | Org manager, in the web app |
| `POST /members/{id}/invite` | Send the invite mail again. | Org manager, in the web app |
| `GET /grants`, `POST /grants` | The grants in force; hand a capability to someone for a filter inside your own. | Anyone sees their own; grant on the filter, in the web app |
| `GET /cutoff`, `POST /cutoff` | The cutoff; move it. | Anyone; org manager, in the web app |
| `GET /submissions`, `POST /submissions` | Submissions you may see; submit your messages since your last submit. | Anyone, for themselves |
| `POST /submissions/{id}/approve`, `/reject` | Approve, or reject with a reason. | Approve on a filter its messages are in, in the web app |
| `GET /submit-requests`, `POST /submit-requests` | The open requests to submit; ask everyone to submit by a day. | Anyone; org manager, in the web app |
| `GET /api-tokens`, `POST /api-tokens`, `DELETE /api-tokens/{id}` | Your tokens; create one; revoke one. | Anyone for their own, in the web app |
| `POST /api-tokens/{id}/hand-over` | Move a service token to another member. | Its owner or an org manager, in the web app |
| `GET /price-table`, `POST /price-table/rows` | The rows in force; add a row of your own or a contract's own price. | Anyone; org manager, in the web app |
| `POST /reversals` | Reverse any event by id. | Whoever recorded it, whoever holds its capability on its filter, or an org manager |

## Organization {#organization}

```json
{
  "id": "6b657374-0000-4000-8000-000000000001",
  "name": "Kestrel Digital",
  "slug": "kestrel",
  "reporting_currency": "USD",
  "approval": true,
  "sign_in_domains": ["kestrel.example"],
  "keep_content_text": false,
  "cutoff": "2026-02-28",
  "created_at": "2026-02-01T08:50:00Z"
}
```

`PATCH /organization` takes any of `name`, `reporting_currency`, `approval`, `sign_in_domains` and `keep_content_text`; fields left out stay. `keep_content_text` (default `false`) keeps prompts, responses, tool output and file contents in the [received records](/reference/api/messages#received-records); the log never holds them, and no screen shows them. `approval` says whether submissions need approval. `sign_in_domains` limits magic-link sign-in to these email domains; empty means any. `slug` is used in invite links. `cutoff` is the cutoff in force, `null` until one is set. See [Organization settings](/account/organizations#settings).

`GET /organization/member-limit` answers `{"members": 5, "member_limit": 150, "message": null}`; `members` counts everyone invited or joined who has not left, `member_limit` is `null` where there is none, as on the open edition, and `message` is what an invite past the limit is refused with. See [Plans and limits](/plans#members).

## Members {#members}

```http
POST /api/v1/members

{ "name": "Mara Lind", "email": "mara@kestrel.example", "role": "agent_manager", "attributes": { "team": "web" } }
```

`role` is `agent_manager` (the default), `people_manager` or `org_manager`; a people manager also takes `filters`, at least one, the filters their role is granted on, such as `[{"team": "web"}]`. `201`:

```json
{
  "member": {
    "id": "7f1c4e2a-…",
    "email": "mara@kestrel.example",
    "name": "Mara Lind",
    "role": "agent_manager",
    "attributes": { "team": "web" },
    "filters": [],
    "status": "invited",
    "invited_at": "2026-02-01T09:20:00Z",
    "joined_at": null
  },
  "invite_url": "https://app.tokencontroller.com/i/kestrel-4f2a9c1e7b03"
}
```

The person gets the link by mail; `tc connect` takes it too. `409` when the address is already a member or the member limit is reached. `GET /members` lists everyone in the `member` shape; `status` is `invited`, `joined` or `removed`, and `attributes` are theirs today. `PATCH /members/{id}` changes `name`, `email`, `role` or `filters`; a new role or new filters replace the grants the old ones gave. `DELETE /members/{id}` removes a member: they can no longer sign in, their past messages keep them as their person, and their service tokens wait for a new owner. `409` when the organization would have no org manager left.

## Grants {#grants}

A grant is (person, capability, filter). Roles give grants too; `POST /grants` hands one out by itself.

```http
POST /api/v1/grants

{ "person": "tom@kestrel.example", "capability": "view", "filter": { "client": "acme" } }
```

`capability` is `view`, `assign`, `approve`, `plan` or `grant`; record is everyone's, and adjust belongs to org managers. The filter must lie inside one you hold grant on. `201`:

```json
{
  "id": "grt_41ab…",
  "person": "tom@kestrel.example",
  "capability": "view",
  "filter": { "client": "acme" },
  "from_role": false,
  "recorded_at": "2026-03-02T10:00:00Z"
}
```

A new grant for the same person and capability supersedes the old one; reversing the `id` takes it away. `GET /grants` lists the grants in force: everyone sees their own, whoever holds grant on a filter sees the grants inside it, org managers see all. `person=tom@kestrel.example` keeps one person's. See [Grants and roles](/account/roles).

## Cutoff {#cutoff}

```json
{
  "cutoff": "2026-02-28",
  "frozen_as_of": "2026-02-28T00:00:00Z",
  "event_id": "cut_8e7d…",
  "moved_by": "nadia@kestrel.example",
  "moved_at": "2026-03-03T08:00:00Z"
}
```

`cutoff` is `null` when none was set: every figure is live. `POST /cutoff` with `{"cutoff": "2026-03-31"}` moves it; moving it back is allowed, and a day after today gets `400`. Figures as of the cutoff never change on their own; events dated before it need adjust. A rule or a contract assignment may still set a contract on messages before it, since figures as of the cutoff stay as they were. See [Cutoff](/account/cutoff).

## Submissions {#submissions}

A submit is a person confirming their own messages since their last submit, up to a day.

```http
POST /api/v1/submissions

{ "to": "2026-03-31" }
```

`to` defaults to today and cannot be later. `201`, here Mara's first submit, which starts at her first message:

```json
{
  "id": "sub_2d9e…",
  "person": "mara@kestrel.example",
  "from": "2026-03-04",
  "to": "2026-03-31",
  "status": "submitted",
  "reason": null,
  "messages": 2,
  "cost": "700.00"
}
```

An empty submit says there was nothing to add. `409` when you already submitted up to that day. `GET /submissions` lists your own and those with messages inside a filter you view; `person=` keeps one person's. `POST /submissions/{id}/approve`, and `/reject` with `{"reason": "wrong client"}`, set `status` to `approved` or `rejected`; a rejected submission goes back to its person's [review list](/attribute/review), and its days are open to submit again. Approving needs approve on a filter the messages are in, never your own. See [Submit](/account/submit).

`POST /submit-requests` with `{"until": "2026-04-03", "message": "March figures go to finance on Friday"}` asks everyone to submit by a day; each person who has not submitted gets one email. `GET /submit-requests` lists the open requests with `id`, `requested_by`, `requested_at`, `until`, `message` and `done`, whether you have submitted up to the request's day.

## API tokens {#api-tokens}

```http
POST /api/v1/api-tokens

{ "kind": "service", "name": "review bot" }
```

`kind` is `service`, `read` or `organization`; anyone creates service and read tokens for themselves, only an org manager the organization token. `201`, the only time the secret is shown:

```json
{
  "id": "3d0a9c51-…",
  "kind": "service",
  "name": "review bot",
  "shown_prefix": "tc_svc_3f9a",
  "person_id": "9b4d…",
  "needs_owner": false,
  "created_at": "2026-02-10T11:00:00Z",
  "last_used_at": null,
  "token": "tc_svc_3f9a…"
}
```

`GET /api-tokens` lists your tokens without their secrets, or every token in the organization for an org manager, CLI tokens (`kind: "cli"`) included. `needs_owner` is set when the person left: the token's entries show as an error until `POST /api-tokens/{id}/hand-over` with `{"person_id": "…"}` gives it a new owner, and only future entries follow. `DELETE /api-tokens/{id}` revokes a token, `204`. See [The person of a message](/account/roles#person).

## Price table {#price-table}

```http
GET /api/v1/price-table?provider=anthropic&model=claude-sonnet-4-5&as_of=2026-03-31
```

```json
{
  "as_of": "2026-03-31T23:59:59.999999999Z",
  "rows": [
    {
      "id": "prc_5f6a…",
      "provider": "anthropic",
      "model": "claude-sonnet-4-5",
      "kind": "input",
      "price_per_million": "10",
      "valid_from": "2026-01-01",
      "source": "organization",
      "contract": null,
      "recorded_at": "2026-02-01T09:00:00Z"
    }
  ]
}
```

The example organization prices each of its models with one row of its own, so the worked month costs the same on every instance. Rows are sorted by provider, model, `valid_from` and kind. `source` is `managed` (ids `prm_…`) or `organization` (ids `prc_…`); the organization's row wins over a managed row with the same provider, model, kind and `valid_from`. `price_per_million` is in US dollars, written in full without trailing zeros, such as `"0.3"`. `contract` names the contract whose own price this is: such a row counts for that contract's messages only and wins over every other row there.

`POST /price-table/rows` takes `provider`, `model`, `kind`, `price_per_million` and `valid_from`, and optionally `contract`:

```http
POST /api/v1/price-table/rows

{ "provider": "anthropic", "model": "claude-sonnet-4-5", "kind": "input", "price_per_million": "9", "valid_from": "2026-04-01", "contract": "anthropic-api" }
```

`provider` is one the price table names rows under: `anthropic`, `openai`, `google`, `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or `other`. `kind` is any token kind, such as `input`, `output`, `cache_read`, `cache_write`, `cache_write_5m`, `cache_write_1h` or `reasoning`. `contract` must be a contract on record with the same provider. `201` with the row; `409` when the same row is already in force. A wrong row is reversed by its `id`; managed rows cannot be reversed. The columns are on [Price tables](/reference/pricing-tables).

## Reversals {#reversals}

```http
POST /api/v1/reversals

{ "event_id": "spl_0c3a…" }
```

`201` with `id`, the reversal's own event id (`rev_…`), and `event_id`. The event stays in the log and stops counting; a reversed split leaves the earlier split, or the rules, in force again, and a reversed contract assignment lets the message find its contract again. `dated` dates the reversal; before the cutoff that needs adjust. `409` when the event is already reversed, for a reversal itself (record the event again instead), and for a managed price row. Whoever recorded the event, whoever holds the capability it took on its filter, or an org manager may reverse it; a service token may reverse what it recorded. A saved report is removed the same way, by its `rpt_…` id; the answer then gives that id as both `id` and `event_id`.
