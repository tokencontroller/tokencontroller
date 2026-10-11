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

## Main endpoints {#endpoints}

| Endpoint | What it does |
| --- | --- |
| `POST /token-entries` | Report a token entry. See [Token entries endpoint](/collect/token-entries). |
| `POST /bills` | Send a bill for a period. See [Bill schema](/reference/bill). |
| `GET /reports/…` | Read reports. See [Reporting API](/report/reporting-api). |

Everything the web app does goes through this API.
