---
title: "HTTP API"
description: "Tokens and main endpoints of the versioned REST API."
---

A versioned REST API under `/api/v1`. The full description is an OpenAPI document at `/api/v1/openapi.json`.

## Tokens {#auth}

| Token | For |
| --- | --- |
| `tc_svc_…` | Service tokens for agents that report token entries. The creator is the agent manager. |
| `tc_read_…` | Read tokens for reports. They see what their creator's role sees. |
| `tc_org_…` | The organization token for telemetry push. |

Send a token in the `Authorization` header, such as `Authorization: Bearer tc_read_…`. The web app signs in with a magic link and keeps a session cookie. `tc connect` signs in with a magic link too, and keeps its `tc_cli_…` token on the machine.

## Main endpoints {#endpoints}

| Endpoint | What it does |
| --- | --- |
| `POST /token-entries` | Report a token entry. See [Token entries endpoint](/collect/token-entries). |
| `POST /token-entries/csv/preview`, `POST /token-entries/csv` | Read a CSV file and show the problem in each row, then import it as drafts. See [CSV import](/collect/manual#csv). |
| `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` | OpenTelemetry from agents, with the organization token. See [Telemetry](/collect/telemetry). |
| `POST /bills` | Send a bill for a period. See [Bill schema](/reference/bill). |
| `GET /reports/…` | Read reports. See [Reporting API](/report/reporting-api). |
| `GET /me/context` | What the CLI needs to place token entries like the service does: the organization's rules and tag names, your own rules and tags, and the projects with their ticket key patterns and repositories. `PUT /me/rules` replaces your own rules. |
| `GET /status` | The state of each part of the service, without signing in. status.tokencontroller.com shows it. |

Everything the web app does goes through this API.

## MCP server {#mcp}

`/api/v1/mcp` is a Model Context Protocol server, so an AI agent can read your organization: its settings and tag map, members, projects, clients and tickets. It only reads, and it sees what the token's creator sees. The people who run the instance (`TC_OPERATOR_EMAILS`) also get tools for every organization's customer record, support waiting for a person, site assistant questions the docs did not answer, and unanswered feedback reports. Connect Claude Code with a read token:

```bash
claude mcp add --transport http token-controller \
  https://app.tokencontroller.com/api/v1/mcp \
  --header "Authorization: Bearer tc_read_…"
```
