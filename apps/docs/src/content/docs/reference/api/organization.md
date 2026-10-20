---
title: "Organization and people"
description: "Settings, members and invites, grants, the cutoff, submissions and submit requests, API tokens, the price table and reversals."
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
| `GET /grants`, `POST /grants` | The grants in force; hand a capability to someone for a filter inside your own. | Anyone sees their own; grant on the filter |
| `GET /cutoff`, `POST /cutoff` | The cutoff; move it. | Anyone; org manager, in the web app |
| `GET /submissions`, `POST /submissions` | Submissions you may see; submit your messages since your last submit. | Anyone, for themselves |
| `POST /submissions/{id}/approve`, `/reject` | Approve, or reject with a reason. | Approve on a filter its messages are in |
| `GET /submit-requests`, `POST /submit-requests` | The open requests to submit; ask everyone to submit by a day. | Anyone; org manager, in the web app |
| `GET /api-tokens`, `POST /api-tokens`, `DELETE /api-tokens/{id}` | Your tokens; create one; revoke one. | Anyone for their own, in the web app |
| `POST /api-tokens/{id}/hand-over` | Move a service token to another member. | Its owner or an org manager, in the web app |
| `GET /price-table`, `POST /price-table/rows` | The rows in force; add a row or an override. | Anyone; org manager, in the web app |
| `POST /reversals` | Reverse any event by id. | Whoever recorded it, whoever holds its capability on its filter, or an org manager |

## Organization {#organization}

```json
{
  "id": "…",
  "name": "Kestrel Digital",
  "slug": "kestrel",
  "reporting_currency": "USD",
  "approval": true,
  "cutoff": "2026-02-28",
  "sign_in_domains": ["kestrel.example"],
  "created_at": "2026-01-08T08:50:00Z"
}
```

`PATCH /organization` takes any of `name`, `reporting_currency`, `approval` and `sign_in_domains`; fields left out stay. `approval` says whether submissions need approval. `sign_in_domains` limits magic-link sign-in to these email domains; empty means any. `slug` is used in invite links. See [Organization settings](/account/organizations).

`GET /organization/member-limit` answers `{"members": 12, "member_limit": 150, "message": null}`; `member_limit` is `null` where there is none, as on the open edition, and `message` is what an invite past the limit is refused with. See [Plans and limits](/plans#members).

## Members {#members}

```http
POST /api/v1/members

{ "name": "Mara Lind", "email": "mara@kestrel.example", "role": "agent_manager", "attributes": { "team": "web" } }
```

`role` is `agent_manager` (the default), `people_manager` or `org_manager`; a people manager also takes `filters`, the filters their role is granted on, such as `[{"team": "web"}]`. `201`:

```json
{
  "invite_url": "https://app.tokencontroller.com/i/kestrel-4f2a9c1e7b03",
  "member": {
    "id": "7f1c4e2a-…",
    "name": "Mara Lind",
    "email": "mara@kestrel.example",
    "role": "agent_manager",
    "filters": [],
    "attributes": { "team": "web" },
    "status": "invited",
    "invited_at": "2026-01-08T09:20:00Z",
    "joined_at": null
  }
}
```

The person gets the link by mail; `tc connect` takes it too. `409` when the address is already a member or the member limit is reached. `GET /members` lists everyone in this shape; `status` is `invited`, `joined` or `removed`. `PATCH /members/{id}` changes `name`, `email`, `role` or `filters`; a new role or new filters replace the grants the old ones gave. `DELETE /members/{id}` removes a member: they can no longer sign in, their past messages keep them as their person, and their service tokens wait for a new owner. `409` when the organization would have no org manager left.

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
  "moved_at": "2026-03-03T08:00:00Z",
  "moved_by": "nadia@kestrel.example"
}
```

`cutoff` is `null` when none was set: every figure is live. `POST /cutoff` with `{"cutoff": "2026-03-31"}` moves it; moving it back is allowed. Figures as of the cutoff never change on their own; events dated before it need adjust. See [Cutoff](/account/cutoff).

## Submissions {#submissions}

A submit is a person confirming their own messages since their last submit, up to a day.

```http
POST /api/v1/submissions

{ "to": "2026-03-31" }
```

`to` defaults to today and cannot be later. `201`:

```json
{
  "id": "sub_2d9e…",
  "person": "mara@kestrel.example",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "messages": 24,
  "cost": "700.00",
  "status": "submitted",
  "reason": null
}
```

An empty submit says there was nothing to add. `409` when you already submitted up to that day. `GET /submissions` lists your own and those with messages inside a filter you view; `person=` keeps one person's. `POST /submissions/{id}/approve` and `/reject` with `{"reason": "wrong client"}` set `status` to `approved` or `rejected`; a rejected submission goes back to its person's check list. Approving needs approve on a filter the messages are in, never your own. See [Submit](/account/submit).

`POST /submit-requests` with `{"until": "2026-04-03", "message": "March closes on Friday"}` asks everyone to submit by a day, the way finance asks for timesheets; each person who has not submitted gets one email. `GET /submit-requests` lists the open requests with `done`, whether you have submitted up to the request's day.

## API tokens {#api-tokens}

```http
POST /api/v1/api-tokens

{ "kind": "service", "name": "review bot" }
```

`kind` is `service`, `read` or `organization`; anyone creates service and read tokens for themselves, only an org manager the organization token. `201`, the only time the secret is shown:

```json
{
  "id": "…",
  "kind": "service",
  "name": "review bot",
  "token": "tc_svc_3f9a…",
  "shown_prefix": "tc_svc_3f9a",
  "person_id": "9b4d…",
  "needs_owner": false,
  "created_at": "2026-01-10T11:00:00Z",
  "last_used_at": null
}
```

`GET /api-tokens` lists your tokens without their secrets, or every token in the organization for an org manager, CLI tokens included. `needs_owner` is set when the person left: the token's entries show as an error until `POST /api-tokens/{id}/hand-over` with `{"person_id": "…"}` gives it a new owner, and only future entries follow. `DELETE /api-tokens/{id}` revokes a token, `204`. See [The person of a message](/account/roles#person).

## Price table {#price-table}

```http
GET /api/v1/price-table?provider=anthropic&model=claude-sonnet-5&as_of=2026-03-31
```

```json
{
  "as_of": "2026-03-31T23:59:59Z",
  "rows": [
    { "id": "prm_…", "provider": "anthropic", "model": "claude-sonnet-5", "kind": "input", "price_per_million": "3.00", "valid_from": "2026-01-01", "source": "managed", "recorded_at": "2026-01-01T00:00:00Z" },
    { "id": "prm_…", "provider": "anthropic", "model": "claude-sonnet-5", "kind": "output", "price_per_million": "15.00", "valid_from": "2026-01-01", "source": "managed", "recorded_at": "2026-01-01T00:00:00Z" },
    { "id": "prc_5f6a…", "provider": "anthropic", "model": "claude-sonnet-5", "kind": "cache_read", "price_per_million": "0.25", "valid_from": "2026-03-01", "source": "organization", "recorded_at": "2026-03-01T09:00:00Z" }
  ]
}
```

`source` is `managed` or `organization`; the organization's row wins over a managed row with the same provider, model, kind and `valid_from`. `POST /price-table/rows` takes `provider`, `model`, `kind` (`input`, `output`, `cache_read` or `cache_write`), `price_per_million` in US dollars and `valid_from`; `409` when the same row is already in force. A wrong row is reversed by its `id`; managed rows cannot be reversed. The columns are on [Price tables](/reference/pricing-tables).

## Reversals {#reversals}

```http
POST /api/v1/reversals

{ "event_id": "spl_0c3a…" }
```

`201` with `id`, the reversal's own event id (`rev_…`), and `event_id`. The event stays in the log and stops counting; a reversed split leaves the earlier split, or the rules, in force again. `dated` dates the reversal; before the cutoff that needs adjust. `409` when the event is already reversed. Whoever recorded the event, whoever holds the capability it took on its filter, or an org manager may reverse it; a saved report is reversed the same way. A service token may reverse what it recorded.
