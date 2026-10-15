---
title: "HTTP API"
description: "Tokens, main endpoints and the MCP server of the versioned REST API."
---

A versioned REST API under `/api/v1`; everything the web app does goes through it. The full description is the OpenAPI document at `/api/v1/openapi.json`.

## Tokens {#auth}

Send a token in the `Authorization` header, such as `Authorization: Bearer tc_read_…`.

| Token | For |
| --- | --- |
| `tc_svc_…` | Service tokens for agents that report token entries. The creator is the agent manager. |
| `tc_read_…` | Read tokens for reports. They see what their creator's role sees. |
| `tc_org_…` | The organization token for telemetry push. |
| `tc_cli_…` | Kept on the machine by `tc connect`, which signs in with a magic link. |

The web app signs in with a magic link and keeps a session cookie.

## Main endpoints {#endpoints}

Paths are under `/api/v1`.

| Endpoint | What it does |
| --- | --- |
| `POST /token-entries` | Report one token entry or a list of them. See [Token entries endpoint](/collect/token-entries). |
| `POST /token-entries/csv/preview`, `POST /token-entries/csv` | Read a CSV file and show the problem in each row, then import it as drafts. See [CSV import](/collect/manual#csv). |
| `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` | OpenTelemetry from agents, with the organization token. See [Telemetry](/collect/telemetry). |
| `/ticket-systems` | Connect Jira Cloud, Azure DevOps or GitHub Issues, map their projects and sync now. See [Projects and tickets](/account/projects#ticket-systems). |
| `POST /bills` | Send a bill for a period. See [Bill schema](/reference/bill). |
| `POST /bills/csv/preview`, `POST /bills/csv` | Read a CSV bill file or a cloud's cost export (`?cloud=bedrock&period=2026-09`) and show what it holds, then upload it. See [CSV bills](/account/bills#csv). |
| `/bill-connections` | Connect Anthropic Enterprise, Anthropic Console or OpenAI with an admin key, and pull now. See [Bills](/account/bills#connect). |
| `GET /reports/…` | Read reports. See [Reports](#reports) below and the [reporting API](/report/exports#reporting-api). |
| `GET /exports/raw` | Everything the organization has, as a zip of JSON files. Organization managers only. See [Raw export](/report/exports#raw-export). |
| `GET /me/context` | What the CLI needs to place token entries like the service does: the organization's rules and tag names, your own rules and tags, and the projects with their ticket key patterns and repositories. `PUT /me/rules` replaces your own rules. |
| `GET /status` | The state of each part of the service, without signing in. status.tokencontroller.com shows it. |

## Reports {#reports}

Every report answers within the caller's role. Paths are under `/api/v1`.

| Endpoint | What it returns |
| --- | --- |
| `GET /reports/cost` | Cost grouped by any tag, within a filter. The main report. |
| `GET /reports/agent-harness` | Cost by agent harness, agent name, model, effort or speed. |
| `GET /reports/outliers` | Tickets that cost far more or less than similar past tickets. `limit` sets how many (default 20). |
| `GET /reports/token-budgets` | Token budget vs actual: one row per token budget in force in the period. Token budgets overlap, so the rows are never added up. |
| `GET /reports/token-budgets/{id}` | One token budget opened down to the tickets and token entries behind its actual. With `format=csv`, `table` is `tickets` (the default) or `token_entries`. |
| `GET /reports/token-entries` | Token entries with their splits, tags and summaries. `person` limits it to one agent manager; `limit` (up to 5000) and `offset` page through it. The CSV has one row per split. |
| `GET /reports/saved`, `POST /reports/saved` | List saved reports, or save a tag filter with a grouping as a report. |
| `GET /reports/saved/{id}`, `DELETE /reports/saved/{id}` | A saved report for one period, or remove it (its creator or an organization manager). |
| `GET /reconciliation` | Each bill of a period against its token entries, with residual. |
| `GET /periods/{period}/cost-sheet` | The cost sheet of a period: final once closed, a preview while open. `columns` adds tag names as columns, such as `team,purpose`. |

| Parameter | Meaning |
| --- | --- |
| `period` | The period, such as `2026-09`. Required for cost, agent harness, outliers, saved reports and reconciliation. Left out, token budgets use this month and token entries all time. |
| `by` | A tag name, or `agent_harness`, `agent_name`, `model`, `effort`, `speed`, `day` or `cost_kind`. |
| `filter` | A tag filter, such as `client:acme,team:web`. |
| `status` | `all` (the default) or `approved`. For token entries: a comma-separated list out of `submitted`, `approved`, `rejected` and `locked`; left out, every status that counts. |
| `currency` | Show money in another currency, such as `USD`. |
| `view` | `money` (the default) or `share`. |
| `format` | `csv` for a CSV file instead of JSON. |

## MCP server {#mcp}

`/api/v1/mcp` is a Model Context Protocol server, so an AI agent can read your organization: its settings and tag map, members, projects, clients and tickets. It only reads, and it sees what the token's creator sees. Connect Claude Code with a read token:

```bash
claude mcp add --transport http token-organization manager \
  https://app.tokencontroller.com/api/v1/mcp \
  --header "Authorization: Bearer tc_read_…"
```

## Estimates API {#estimates}

Ask what described work will cost before it starts. No recording is needed: any token can ask.

```json
POST /api/v1/estimates
{
  "work": "Refund checkout for a returns portal",
  "model": "claude-opus-5-5",
  "context": "Last flow: 702.40 USD, sandbox timeouts"
}
```

`work` is the description: a prompt, a spec, a ticket or acceptance criteria. `context` is optional: anything that helps, such as the summaries and cost of earlier work. The answer is a probability distribution in the organization's reporting currency:

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

Each estimate is billed once, whatever the length of `work` and `context`, so send all the context you have. `PATCH /estimates/{id}` links an estimate to a ticket or project; token entries placed there give its [delta](/account/token-budgets#delta). `GET /estimates/{id}` returns it with its actual cost and delta.
