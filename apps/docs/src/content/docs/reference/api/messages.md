---
title: "Messages and sessions"
description: "Report usage with token entries, import CSV, send OpenTelemetry, read messages and sessions with their cost, contract and tags, split a session, set its contract, and manage received records."
---

A message is one model call with its token counts; a session is the messages of one agent run. Any agent reports them with one call per session. Every usage endpoint first stores what it received, then reads it into the log, and a message seen through two usage sources is one record.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /token-entries` | Report one message or session record, or a list of up to 1,000. | Service token, or a signed-in person for their own sessions |
| `POST /token-entries/csv/preview` | Read a CSV file and show what it holds, with the problem in each row that cannot be read. Saves nothing. | Signed in |
| `POST /token-entries/csv` | Import a CSV file as your own messages. Every row must be readable, or nothing is saved. | Signed in |
| `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` | OpenTelemetry from agents. | Organization token |
| `GET /messages` | Messages with their cost, contract, parts, tags, signals and where they stand in submit and approval. | View on the filter |
| `GET /sessions` | Sessions: their messages added up, with the cost by tag set and the contracts. | View on the filter |
| `POST /splits` | Split a session, or one message of it, over tag sets. | Your own; assign on a filter the session is in |
| `POST /contract-assignments` | Put whole sessions under one contract, by hand. | Your own; assign on a filter each session is in |
| `GET /received-records` | What the usage endpoints received, without the bodies. | Org manager, in the web app |
| `DELETE /received-records`, `DELETE /received-records/{id}` | Delete received records for good. | Org manager, in the web app |

## Token entries {#token-entries}

A token entry is one message, or one session with its messages. The fields are the [token entry schema](/reference/token-entry); the walkthrough is [Token entries endpoint](/collect/token-entries).

```http
POST /api/v1/token-entries
Authorization: Bearer tc_cli_…

{
  "agent_harness": "claude-code",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5",
  "session_id": "S1",
  "started_at": "2026-03-04T09:20:00Z",
  "ended_at": "2026-03-04T10:00:00Z",
  "tokens": { "input": 40000000 },
  "summary": "Checkout: saved addresses",
  "facts": { "turns": 12, "tool_calls": 30 },
  "feature": "KD-1",
  "signals": {
    "branch": "KD-1-saved-addresses",
    "repository": "github.com/kestrel/shop",
    "organization.id": "kestrel-team"
  }
}
```

`provider` is who charges for the call, spelled as bills spell it. Leave it out when you do not know it: it is never guessed from the model's name, and the message's contract names its provider later. `tokens` takes any kind by its name, such as `{"input": 1200, "output": 300, "cache_write_1h": 5000}`; `cache_write` is a cache write of unknown duration, priced as 5 minutes. `signals` is an open list: any name beyond the named ones, such as `organization.id`, is kept as it came, and a signal named like a tag, such as `project`, is your own assignment and wins over the organization's rules. A `contract` in `tags` puts the message under that contract. See [Signals](/reference/data-model#signals) and [Token kinds](/reference/data-model#token-kinds).

`201 Created` with one object per token entry, or a list for a list:

```json
{
  "session_id": "S1",
  "messages": 1,
  "cost": "400.00",
  "currency": "USD",
  "label": "direct",
  "unknown_tags": []
}
```

`cost` is the token counts times the [price table](/account/price-table), as of now, in US dollars. `label` is `direct`, `project_overhead` or `organization_overhead`, from the entry's largest part. `unknown_tags` lists the new tag values the entry named, as `name: key`; they count as sent, and someone with assign records them. An entry with no token kind at all is stored as a count error with no cost, never as zero. An entry without `session_id` is counted under its `local_id`, else under a new id the answer names.

A list is all or nothing: up to 1,000 entries per call, and an entry that breaks the schema gets `400` naming it, with nothing of the list in the log. Sending an entry again with the same `session_id` and `message_id`, or the same `local_id`, adds nothing. The person of every message is the token's creator; a signed-in person reports their own. `429` when the organization reached its [daily limit](/plans#messages) of messages or of bytes received; nothing of the call was stored.

## CSV import {#csv}

The same fields as columns, with a dot for nested fields, as [CSV import](/collect/manual#csv) describes. Send the file as the body with `Content-Type: text/csv`, up to 10,000 rows. Preview first:

```json
{ "ready": 41, "problems": [ { "line": 7, "problem": "tokens.output \"12k\" is not a whole number of tokens" } ] }
```

`POST /token-entries/csv` answers `201` with `token_entries`, one [stored entry](#token-entries) per row, or `400` naming the first row that cannot be read, and then nothing is saved.

## OpenTelemetry {#otlp}

Agents send every model call to `https://app.tokencontroller.com/api/v1/ingest/otlp`, with the organization token in `Authorization: Bearer tc_org_…`. The settings per agent are on [OpenTelemetry](/collect/telemetry).

| Path | Takes |
| --- | --- |
| `POST /ingest/otlp/v1/logs` | One model call per log event: Claude Code, Codex CLI, Gemini CLI, Cursor. Other events of these agents are kept as activities. |
| `POST /ingest/otlp/v1/traces` | One model call per chat span: Copilot CLI. Other spans are kept as activities. |
| `POST /ingest/otlp/v1/metrics` | Counters such as commits and lines changed, kept as activities. Token and cost counters are left out, since the events already count every call. |

OTLP over HTTP, as `application/x-protobuf` or `application/json`, plain or `Content-Encoding: gzip`. gRPC is not taken. The answer is the OTLP export response of that signal; `415` for another content type or encoding, `413` for a body that is too large, `400` for one that cannot be decoded, `429` past the daily limit. Each export is stored as a [received record](#received-records) before it is read. Content text (prompts, responses, tool output, file contents) is removed on arrival unless the organization keeps it; nothing is refused for it, and the answer's `errorMessage` says how many records lost some. Resource attributes arrive as [signals](/collect/telemetry#tags), so rules can place the cost and find the contract.

## Messages {#messages}

```http
GET /api/v1/messages?from=2026-03-01&to=2026-03-31&session_id=S2&as_of=2026-04-05
```

| Parameter | Meaning |
| --- | --- |
| `from`, `to` | The date range. Default: everything. |
| `as_of` | The day or moment up to which the log is read. Default now. |
| `filter` | Such as `client:acme,team:web`. Answers inside your view grants. |
| `approved` | `true` for approved messages only. |
| `session_id` | One session. |
| `unassigned` | `true` for messages with no feature and no project: organization overhead, what the [review list](/attribute/review) asks about. |
| `limit` | At most this many, newest first. Default 200. |

```json
{
  "as_of": "2026-04-05T23:59:59.999999999Z",
  "messages": [
    {
      "session_id": "S2",
      "message_id": "1",
      "person": "mara@kestrel.example",
      "provider": "openai",
      "model": "gpt-5",
      "contract": "openai-api",
      "contract_by": "fallback",
      "on_a_plan": false,
      "at": "2026-03-12T14:00:00Z",
      "agent_harness": "codex",
      "agent_name": null,
      "tokens": { "cache_read": 0, "cache_write": 0, "input": 30000000, "output": 0 },
      "cost": "300.00",
      "cost_label": "priced",
      "reported_cost": null,
      "assigned_by": "hand",
      "parts": [
        {
          "fraction": "0.6",
          "assigned": { "feature": "KD-1" },
          "tags": { "client": "acme", "contract": "openai-api", "department": "engineering", "feature": "KD-1", "person": "mara@kestrel.example", "project": "shop", "team": "web" },
          "amount": "180.00",
          "label": "direct"
        },
        {
          "fraction": "0.4",
          "assigned": { "feature": "KD-2" },
          "tags": { "client": "acme", "contract": "openai-api", "department": "engineering", "feature": "KD-2", "person": "mara@kestrel.example", "project": "shop", "team": "web" },
          "amount": "120.00",
          "label": "direct"
        }
      ],
      "signals": {
        "branch": "KD-1-payment-component",
        "working_directory": null,
        "repository": "github.com/kestrel/shop",
        "pull_request": null,
        "pull_request_title": null,
        "subagent_type": null,
        "prompt_keys": [],
        "config": {},
        "resource": {}
      },
      "facts": {
        "turns": 12,
        "tool_calls": 30,
        "retries": null,
        "reruns": null,
        "duration_seconds": null,
        "subagents": null,
        "files_touched": null,
        "result": null,
        "summary": "Shared payment component for checkout and returns"
      },
      "submitted": "submitted"
    }
  ]
}
```

`provider` is the provider of the message's contract, else the one the sender reported, else `null`. `contract` is the message's contract, if it has one, and `contract_by` how it got it: `hand`, `sender`, `rule`, `project_default` or `fallback` (the provider has only one contract). Here OpenAI has one contract, so S2 got `openai-api` from the fallback. `on_a_plan` is `true` when the contract is flat, so a fee pays the message's cost. `tokens` holds every kind the message has, such as `cache_write_1h`; `input`, `output`, `cache_read` and `cache_write` are always written, zero or not.

`cost_label` is `priced`, `count_error` or `not_priced_yet`; with the last two, `cost` is `null`, never zero. A message is not priced yet when no price row fits for certain, such as a model with no provider that providers price differently; see [Not priced yet](/reference/pricing-tables#unknown). Each part of `parts` is one piece of the message's split: `assigned` is what the split said, `tags` the tag set with its [defaults](/account/tags#defaults) added, with the contract, the person and their attributes, which is what filters and reports see. `assigned_by` is `hand`, `rule` or `default`. `signals` holds the named signals, written even when empty, and any other signal by its name. `submitted` is `open`, `submitted`, `approved` or `rejected`.

## Sessions {#sessions}

`GET /sessions` takes the same parameters and returns `sessions`, each with its messages added up:

```json
{
  "as_of": "2026-04-05T23:59:59.999999999Z",
  "sessions": [
    {
      "session_id": "S4",
      "person": "tom@kestrel.example",
      "agent_harness": "claude-code",
      "agent_name": null,
      "started_at": "2026-03-25T11:00:00Z",
      "ended_at": "2026-03-25T11:00:00Z",
      "messages": 1,
      "cost": "150.00",
      "count_errors": 0,
      "not_priced_models": [],
      "contracts": ["anthropic-api"],
      "contract_by": ["hand"],
      "no_contract": 0,
      "provider_unknown": 0,
      "parts": [
        { "fraction": "1", "assigned": {}, "tags": { "contract": "anthropic-api", "department": "engineering", "person": "tom@kestrel.example", "team": "web" }, "amount": "150.00", "label": "organization_overhead" }
      ],
      "summary": "Exploring a logging library",
      "signals": { "branch": null, "working_directory": null, "repository": null, "pull_request": null, "pull_request_title": null, "subagent_type": null, "prompt_keys": [], "config": {}, "resource": {} },
      "facts": { "turns": 12, "tool_calls": 30, "retries": null, "reruns": null, "duration_seconds": null, "subagents": null, "files_touched": null, "result": null, "summary": "Exploring a logging library" },
      "assigned_by": "default",
      "submitted": "open"
    }
  ]
}
```

`started_at` and `ended_at` are the times of the session's first and last message. `cost` adds up its priced messages; `count_errors` and `not_priced_models` name what is left out. `contracts` are the contracts of its messages, and `contract_by` how they were found, each way once: `hand`, `sender`, `rule`, `project_default` or `fallback`. `no_contract` counts the messages with no contract, which the [review list](/attribute/review) asks about, and `provider_unknown` those of them whose provider is not known either. S4 ran on an API key and Anthropic has two contracts, so Tom answered the review list by hand: `no_contract` was 1 before, and 0 after the [contract assignment](#contract-assignments).

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

`201` with the split:

```json
{
  "id": "spl_0c3a…",
  "session_id": "S2",
  "message_id": null,
  "parts": [
    { "fraction": "0.6", "tags": { "feature": "KD-1" } },
    { "fraction": "0.4", "tags": { "feature": "KD-2" } }
  ],
  "supersedes": null,
  "recorded_at": "2026-03-12T16:00:00Z"
}
```

`supersedes` is the id of the split it replaced. Add `message_id` to split one message; add `dated` to date the split, which before the [cutoff](/account/cutoff#adjust) needs adjust. A split never names `person`, and it leaves the contract as it is. Your own sessions need nothing more; someone else's need assign on a filter the session is in. `404` when the session or message is not inside what you may view.

## Contract assignments {#contract-assignments}

When a message cannot find its contract by a rule, a project default or the fallback, the review list asks which contract it is under. The answer is a contract assignment on whole sessions:

```http
POST /api/v1/contract-assignments

{ "contract": "anthropic-api", "sessions": ["S4"] }
```

`201` with `{"contract": "anthropic-api", "sessions": 1}`, the number of sessions that now carry it. `contract` must be a contract the organization has, from [`GET /contracts`](/reference/api/bills#contracts); `400` otherwise, and for an empty list or more than 1,000 sessions. Every message of each session gets the contract, which wins over every rule; a later assignment on the same session supersedes it, and it is reversed by its id (`con_…`) from the [event log](/reference/api/reports#events). It may cover messages before the cutoff, since figures as of the cutoff stay as they were. Your own sessions need nothing more; someone else's need assign on a filter each session is in. `404` for a session you may not view. See [Which contract a message gets](/reference/data-model#contract).

## Received records {#received-records}

Every usage endpoint (`POST /token-entries`, the CSV import and the three OpenTelemetry paths) stores what it received before it reads it, with content text removed unless the organization [keeps it](/reference/api/organization#organization). A token entries body is stored even when an entry in it then fails the schema. Messages and activities read from a record carry its id in `received` in the [event log](/reference/api/reports#events). Records are kept for 400 days, then deleted. Org managers only, signed in in the browser.

```http
GET /api/v1/received-records?before=2026-04-01&limit=100
```

```json
[
  {
    "id": "rcv_7d1e…",
    "endpoint": "otlp-logs",
    "received_at": "2026-03-31T17:42:10Z",
    "received_by": "mara@kestrel.example",
    "content_type": "application/x-protobuf",
    "bytes": 48210,
    "stored_bytes": 31877,
    "content_kept": false,
    "delete_after": "2027-05-05T17:42:10Z"
  }
]
```

The latest first, without their bodies. `endpoint` is `token-entries`, `token-entries-csv`, `otlp-logs`, `otlp-traces` or `otlp-metrics`. `received_by` is the person the credential belongs to. `bytes` is the size as it arrived, which the daily limit counts; `stored_bytes` the size after content text was removed. `before` takes a day (its start) or a moment; `limit` defaults to 100, at most 1,000.

`DELETE /received-records/{id}` deletes one record; `DELETE /received-records?before=2026-03-01` deletes every record received before that day or moment, and without `before` all of them. Both answer `{"deleted": 1}` with the number deleted. Deleting a record changes no figure: the messages read from it stay. On a self-hosted instance, `tokencontroller reread` reads an organization's stored telemetry again after an upgrade; see [Configuration](/self-hosting/configuration).
