---
title: "Messages and sessions"
description: "Report usage with token entries, import CSV, push OpenTelemetry, read messages and sessions with their cost and tags, and split a session."
---

A message is one model call with its token counts; a session is the messages of one agent run. Any agent reports them with one call per session. Every door lands in the same log, and a message seen through two doors is one record.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /token-entries` | Report one message or session record, or a list of up to 1,000. | Service token, or a signed-in person for their own sessions |
| `POST /token-entries/csv/preview` | Read a CSV file and show what it holds, with the problem in each row that cannot be read. Saves nothing. | Signed in |
| `POST /token-entries/csv` | Import a CSV file as your own messages. Every row must be readable, or nothing is saved. | Signed in |
| `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` | OpenTelemetry from agents. | Organization token |
| `GET /messages` | Messages with their cost, parts, tags, facts and where they stand in submit and approval. | View on the filter |
| `GET /sessions` | Sessions: their messages added up, with the cost by tag set, the summary and the facts. | View on the filter |
| `POST /splits` | Split a session, or one message of it, over tag sets. | Your own; assign on a filter the session is in |

## Token entries {#token-entries}

A token entry is one message, or one session with its messages. The fields are the [token entry schema](/reference/token-entry); the walkthrough is [Token entries endpoint](/collect/token-entries).

```http
POST /api/v1/token-entries
Authorization: Bearer tc_svc_…

{
  "agent_harness": "support-desk",
  "agent_name": "review bot",
  "provider": "anthropic",
  "model": "claude-sonnet-5",
  "session_id": "support-desk:1182",
  "started_at": "2026-03-04T09:12:00Z",
  "ended_at": "2026-03-04T09:14:10Z",
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200, "cache_write": 0 },
  "reported_cost": "0.031",
  "summary": "Answered refund question for order 1182",
  "feature": "KD-1",
  "signals": { "repository": "github.com/kestrel/shop", "branch": "KD-1-refunds" }
}
```

`201 Created` with one object per token entry, or a list for a list:

```json
{
  "session_id": "support-desk:1182",
  "messages": 1,
  "cost": "0.03",
  "currency": "USD",
  "label": "direct",
  "unknown_tags": []
}
```

`cost` is the token counts times the [price table](/account/price-table), as of now. `label` is `direct`, `project_overhead` or `organization_overhead`, from the entry's largest part. `unknown_tags` lists tags the entry named that are no value on record yet, as `name: key`; they count as sent, and someone with assign creates the value. A message without counts is stored as a count error with no cost. Only a body that is not a token entry at all gets `400`, and then nothing in the list is stored.

A list is all or nothing: up to 1,000 entries per call. Sending an entry again with the same `session_id` and `message_id`, or the same `local_id`, adds nothing. The person of every message is the token's creator; a signed-in person reports their own.

## CSV import {#csv}

The same fields as columns, with a dot for nested fields, as [CSV import](/collect/manual#csv) describes. Send the file as the body with `Content-Type: text/csv`, up to 10,000 rows. Preview first:

```json
{ "ready": 41, "problems": [ { "line": 7, "problem": "tokens.output is missing" } ] }
```

`POST /token-entries/csv` answers `201` with `token_entries`, one [stored entry](#token-entries) per row, or `400` naming the first row that cannot be read, and then nothing is saved.

## OpenTelemetry ingest {#otlp}

Agents push every model call to `https://app.tokencontroller.com/api/v1/ingest/otlp`, with the organization token in `Authorization: Bearer tc_org_…`. The settings per agent are on [Telemetry push](/collect/telemetry).

| Path | Takes |
| --- | --- |
| `POST /ingest/otlp/v1/logs` | One model call per log event: Claude Code, Codex CLI, Gemini CLI, Cursor. |
| `POST /ingest/otlp/v1/traces` | One model call per chat span: Copilot CLI. |
| `POST /ingest/otlp/v1/metrics` | Accepted and not counted, so an agent that sends metrics gets no error. The events already count every call. |

OTLP over HTTP, as `application/x-protobuf` or `application/json`, plain or `Content-Encoding: gzip`. gRPC is not taken. The answer is the OTLP export response of that signal; `415` for another content type, `413` for a body that is too large, `400` for one that cannot be decoded. Prompt text is removed from model call events and any other event that carries it is dropped. Resource attributes arrive as [signals](/collect/telemetry#tags), so rules can place the cost.

## Messages {#messages}

```http
GET /api/v1/messages?from=2026-03-01&to=2026-03-31&filter=feature:KD-1&limit=50
```

| Parameter | Meaning |
| --- | --- |
| `from`, `to` | The date range. Default: everything. |
| `as_of` | The date up to which the log is read. Default today. |
| `filter` | Such as `client:acme,team:web`. Answers inside your view grants. |
| `approved` | `true` for approved messages only. |
| `session_id` | One session. |
| `unassigned` | `true` for messages with no feature and no project: organization overhead, the check list. |
| `limit` | At most this many, newest first. Default 200. |

```json
{
  "as_of": "2026-04-05T07:30:00Z",
  "messages": [
    {
      "message_id": "msg-0418",
      "session_id": "support-desk:1182",
      "person": "mara@kestrel.example",
      "provider": "anthropic",
      "model": "claude-sonnet-5",
      "agent_harness": "support-desk",
      "agent_name": "review bot",
      "at": "2026-03-04T09:12:00Z",
      "tokens": { "input": 4120, "output": 610, "cache_read": 18200, "cache_write": 0 },
      "cost": "0.03",
      "cost_label": "priced",
      "reported_cost": "0.031",
      "parts": [
        {
          "fraction": "1",
          "amount": "0.03",
          "assigned": { "feature": "KD-1" },
          "tags": { "feature": "KD-1", "project": "shop", "client": "acme", "person": "mara@kestrel.example", "team": "web", "department": "engineering" },
          "label": "direct"
        }
      ],
      "assigned_by": "hand",
      "submitted": "open",
      "facts": {},
      "signals": { "repository": "github.com/kestrel/shop", "branch": "KD-1-refunds" }
    }
  ]
}
```

`cost_label` is `priced`, `count_error` or `unknown_model`; with the last two, `cost` is `null`, never zero. Each part of `parts` is one piece of the message's split: `assigned` is what the split said, `tags` the tag set closed under [defaults](/account/tags#defaults) with the person and their attributes, which is what filters and reports see. `assigned_by` is `hand`, `rule` or `default`. `submitted` is `open`, `submitted`, `approved` or `rejected`.

## Sessions {#sessions}

`GET /sessions` takes the same parameters and returns `sessions`, each with its messages added up:

```json
{
  "session_id": "S2",
  "person": "mara@kestrel.example",
  "agent_harness": "claude-code",
  "started_at": "2026-03-10T13:02:00Z",
  "ended_at": "2026-03-10T14:40:00Z",
  "messages": 12,
  "cost": "300.00",
  "count_errors": 0,
  "unknown_models": [],
  "parts": [
    { "fraction": "0.6", "amount": "180.00", "assigned": { "feature": "KD-1" }, "tags": { "feature": "KD-1", "project": "shop", "client": "acme", "person": "mara@kestrel.example", "team": "web", "department": "engineering" }, "label": "direct" },
    { "fraction": "0.4", "amount": "120.00", "assigned": { "feature": "KD-2" }, "tags": { "feature": "KD-2", "project": "shop", "client": "acme", "person": "mara@kestrel.example", "team": "web", "department": "engineering" }, "label": "direct" }
  ],
  "assigned_by": "hand",
  "submitted": "submitted",
  "summary": "Refund flow for returns, and the KD-2 address form",
  "facts": { "turns": 18, "tool_calls": 64, "result": "https://github.com/kestrel/shop/pull/412" },
  "signals": { "repository": "github.com/kestrel/shop", "branch": "KD-1-refunds" }
}
```

## Splits {#splits}

A split puts a session, or one message of it, on one or more tag sets. The fractions add up to 1; one part may leave its fraction out and takes the rest. A new split supersedes the earlier one on the same target; the earlier one stays in the log.

```http
POST /api/v1/splits

{
  "session_id": "S2",
  "parts": [
    { "fraction": "0.6", "tags": { "feature": "KD-1" } },
    { "fraction": "0.4", "tags": { "feature": "KD-2" } }
  ]
}
```

`201` with the split: its `id` (`spl_…`), `session_id`, `message_id` when it was on one message, `parts` as above, and `supersedes`, the id of the split it replaced. Add `message_id` to split one message; add `dated` to date the split, which before the [cutoff](/account/cutoff#adjust) needs adjust. Your own sessions need nothing more; someone else's need assign on a filter the session is in. `404` when the session or message is not inside what you may view.
