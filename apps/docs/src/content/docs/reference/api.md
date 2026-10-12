---
title: "HTTP API"
description: "Tokens and main endpoints of the versioned REST API."
---

A versioned REST API under `/api/v1`. The full description is an OpenAPI document at `/api/v1/openapi.json`.

## Tokens {#auth}

| Token | For |
| --- | --- |
| `tc_svc_…` | Service tokens for agents that report token entries. The creator is the agent manager. |
| `tc_cli_…` | CLI tokens from `tc connect`: one person on one machine. They submit that person's token sheet and edit their rules. Revoke them under _Settings_. |
| `tc_read_…` | Read tokens for reports. They see what their creator's role sees. |
| `tc_org_…` | The organization token for telemetry push. |

Send a token in the `Authorization` header, such as `Authorization: Bearer tc_read_…`. The web app signs in with a magic link and keeps a session cookie. `tc connect` signs in with a magic link too, and keeps a session token on the machine.

## Main endpoints {#endpoints}

| Endpoint | What it does |
| --- | --- |
| `POST /token-entries` | Report a token entry. See [Token entries endpoint](/collect/token-entries). |
| `POST /bills` | Send a bill for a period. See [Bill schema](/reference/bill). |
| `GET /reports/…` | Read reports. See [Reporting API](/report/reporting-api). |
| `GET /status` | The state of each part of the service, without signing in. status.tokencontroller.com shows it. |

Everything the web app does goes through this API.

## MCP server {#mcp}

`/api/v1/mcp` is a Model Context Protocol server, so an AI agent can read your organization: its settings and tag map, members, projects, clients and tickets. It only reads, and it sees what the token's creator sees. Connect Claude Code with a read token:

```bash
claude mcp add --transport http token-controller \
  https://app.tokencontroller.com/api/v1/mcp \
  --header "Authorization: Bearer tc_read_…"
```
