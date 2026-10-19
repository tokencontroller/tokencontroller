---
title: "HTTP API"
description: "Tokens, the endpoints by record type, reports and the MCP server of the versioned REST API."
---

A versioned REST API under `/api/v1`; everything the web app does goes through it. The full description is the OpenAPI document at `/api/v1/openapi.json`. Every write is an event; nothing is edited or deleted, and `POST /reversals` cancels any event.

## Tokens {#auth}

Send a token in the `Authorization` header, such as `Authorization: Bearer tc_read_…`.

| Token | For |
| --- | --- |
| `tc_svc_…` | Service tokens for agents that report messages. The creator is the person of every message. |
| `tc_read_…` | Read tokens for reports. They see what their creator's grants cover. |
| `tc_org_…` | The organization token for telemetry push. |
| `tc_cli_…` | Kept on the machine by `tc connect`, which signs in with a magic link. |

The web app signs in with a magic link and keeps a session cookie.

## Source records {#records}

Paths are under `/api/v1`. Every list takes `as_of`; left out, it is today.

| Endpoint | What it does |
| --- | --- |
| `POST /token-entries` | Report one message or session record, or a list. See [Token entries endpoint](/collect/token-entries) and the [schema](/reference/token-entry). |
| `POST /token-entries/csv/preview`, `POST /token-entries/csv` | Read a CSV file and show the problem in each row, then import it. See [CSV import](/collect/manual#csv). |
| `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` | OpenTelemetry from agents, with the organization token. See [Telemetry](/collect/telemetry). |
| `GET /messages`, `GET /sessions` | Messages and sessions with their splits, tags, facts and summaries, inside a filter and range. |
| `POST /bills`, `GET /bills` | Send a bill for a date range; list bills with their reconciliation. See [Bill schema](/reference/bill). |
| `POST /bills/csv/preview`, `POST /bills/csv` | Read a CSV bill or a cloud's cost export (`?cloud=bedrock`) and show what it holds, then upload it. |
| `/bill-connections` | Connect Anthropic Enterprise, Anthropic Console or OpenAI with an admin key, and pull now. See [Bills](/account/bills#connect). |
| `POST /estimates`, `GET /estimates/{id}` | Ask any estimator for an estimate; read one with its cost and deviation. See [Estimates](#estimates). |
| `GET /tags`, `POST /tags/{name}/values` | Tag names and tag values with their defaults and source ids. See [Tags](/account/tags). |
| `/ticket-systems` | Connect Jira, set the sync map and sync now. See [Jira](/account/tags#jira). |
| `GET /attributes`, `POST /attributes/{name}/values`, `POST /people/{id}/attributes` | Attribute names, values with their defaults, and a person's attributes from a date. See [Attributes](/account/attributes). |
| `POST /shares` | A person's fraction of a project between two dates. See [Shares](/account/estimates#shares). |
| `GET /price-table`, `POST /price-table/rows` | The rows in force and your own rows and overrides. See [Price tables](/reference/pricing-tables). |

## Assignments and events {#events}

| Endpoint | What it does |
| --- | --- |
| `POST /splits` | A split on a session or a message. Supersedes the earlier split on the same target. |
| `POST /estimates/{id}/accept` | Make an estimate the plan for its tag set. |
| `POST /grants`, `GET /grants` | (person, capability, filter). See [Grants and roles](/account/roles). |
| `GET /rules`, `POST /rules`, `PUT /me/rules` | Organization rules, and your own. A new rule runs back since the cutoff. |
| `GET /cutoff`, `POST /cutoff` | Read or move the organization's cutoff. Org managers only. |
| `POST /submissions`, `POST /submissions/{id}/approve`, `POST /submissions/{id}/reject` | Submit your messages since your last submit; approve or reject with a reason. |
| `POST /reversals` | Reverse any event by id. Events before the cutoff need adjust. |
| `GET /events` | The log itself, in order, inside what the caller may view. |
| `GET /exports/raw` | The whole log as a zip of JSON files. Org managers only. See [Raw export](/report/exports#raw-export). |
| `GET /me/context` | What the CLI needs to split sessions like the service does: the organization's rules, tag names and values you may see, your own rules and attributes, and the price table. |
| `GET /status` | The state of each part of the service, without signing in. status.tokencontroller.com shows it. |

## Reports {#reports}

Every report answers inside the caller's view grants and carries its `as_of` date.

| Endpoint | What it returns |
| --- | --- |
| `GET /reports/cost` | C(F, w): the cost of a filter over a range, grouped by any tag or attribute name. The main report. |
| `GET /reports/deviation` | Plan, cost and deviation per value of the grouping, with labels. |
| `GET /reports/forecast` | The forecast of the filter for the next range. |
| `GET /reconciliation` | Each bill against the cost of its provider's messages in its range, with the line no message explains. |
| `GET /reports/saved`, `POST /reports/saved` | List saved reports, or save a filter, grouping and range as one. |

| Parameter | Meaning |
| --- | --- |
| `from`, `to` | The date range, in UTC. Required for cost and deviation. |
| `as_of` | The date up to which the log is read. Default today; the cutoff gives the frozen figures. |
| `by` | A tag or attribute name, or `person`, `label`, `agent_harness`, `agent_name`, `model` or `day`. |
| `filter` | A filter, such as `client:acme,team:web`. |
| `approved` | `true` shows approved messages only. |
| `currency` | Show money in another currency, such as `EUR`. |
| `view` | `money` (the default) or `share`. |
| `format` | `csv` for a CSV file instead of JSON. |

Money is a decimal string in its currency, never a float. Fractions are decimal strings that add up to exactly 1.

## MCP server {#mcp}

`/api/v1/mcp` is a Model Context Protocol server, so an AI agent can read your organization: tag values, attributes, people, plans and reports. It only reads, and it sees what the token's creator sees. Connect Claude Code with a read token:

```bash
claude mcp add --transport http token-controller \
  https://app.tokencontroller.com/api/v1/mcp \
  --header "Authorization: Bearer tc_read_…"
```

## Estimates API {#estimates}

Ask what described work will cost before it starts. No recording is needed: any token can ask.

```json
POST /api/v1/estimates
{
  "work": "Refund checkout for a returns portal",
  "tags": { "feature": "KD-1" },
  "markup_percent": 15,
  "context": "Last flow: 702.40 USD, sandbox timeouts"
}
```

`work` is the description: a prompt, a spec, a feature or acceptance criteria. `tags` and `markup_percent` are optional; so is `context`, anything that helps, such as the summaries and cost of earlier work. The answer is a probability distribution in US dollars, markup inside:

```json
{
  "id": "est_4k2q9w",
  "currency": "USD",
  "median": "610.00",
  "distribution": [
    { "up_to": "250.00", "p": 0.04 },
    { "up_to": "500.00", "p": 0.31 },
    { "up_to": "750.00", "p": 0.41 },
    { "up_to": "1000.00", "p": 0.17 },
    { "up_to": null, "p": 0.07 }
  ]
}
```

Each estimate is billed once, whatever the length of `work` and `context`, so send all the context you have. `POST /estimates/{id}/accept` makes it the [plan](/account/estimates#accept). `GET /estimates/{id}` returns it with the cost of its tag set and the labelled [deviation](/report/deviation).
