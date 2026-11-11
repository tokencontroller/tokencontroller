---
title: "HTTP API"
description: "The REST API under /api/v1: base URL, tokens, errors, dates, money and filters, the OpenAPI document and the SDK. Everything the web app does goes through it."
---

Everything Token Controller does is a call to the REST API under `https://app.tokencontroller.com/api/v1`; the web app, the CLI, the Claude Code mod and the GitHub Action use nothing else. A self-hosted instance serves the same API under its own address. The full description is the OpenAPI document at `/api/v1/openapi.json`.

```http
POST https://app.tokencontroller.com/api/v1/token-entries
Authorization: Bearer tc_svc_…
Content-Type: application/json

{
  "agent_harness": "claude-code",
  "agent_name": "review bot",
  "provider": "anthropic",
  "model": "claude-sonnet-4-5",
  "session_id": "S3",
  "started_at": "2026-03-18T09:00:00Z",
  "ended_at": "2026-03-18T09:40:00Z",
  "tokens": { "input": 10000000 },
  "project": "shop"
}
```

<div class="cards">
  <a class="card" href="/reference/api/messages"><i></i><b>Messages and sessions</b><span>Report usage from any agent, telemetry, splits, contracts.</span></a>
  <a class="card b" href="/reference/api/reports"><i></i><b>Reports and the event log</b><span>Cost, deviation, forecast, the log itself, the raw export.</span></a>
  <a class="card s" href="/reference/api/estimates"><i></i><b>Estimates and plans</b><span>What described work will cost, accept, shares.</span></a>
</div>

| Page | Endpoints |
| --- | --- |
| [Messages and sessions](/reference/api/messages) | Token entries, CSV import, OpenTelemetry, messages, sessions, splits, contract assignments, received records |
| [Bills](/reference/api/bills) | Bills, CSV upload, reconciliation, contracts, bill connections |
| [Estimates and plans](/reference/api/estimates) | Estimates, accept, shares |
| [Tags, attributes and rules](/reference/api/tags) | Tag names and values, attributes, a person's attributes, rules, Jira, the CLI's context |
| [Reports and the event log](/reference/api/reports) | Cost, deviation, forecast, saved reports, events, raw export |
| [Organization and people](/reference/api/organization) | Settings, members, grants, cutoff, submissions, API tokens, price table, reversals |
| [Sign-in and status](/reference/api/sign-in) | Magic links, sign-up, who am I, health, status |
| [MCP server](/reference/api/mcp) | Read-only tools for AI agents |

## Tokens {#auth}

Send a token in the `Authorization` header, such as `Authorization: Bearer tc_read_…`. Create tokens in the web app under _Settings_, or with `POST /api-tokens`. A token is shown once, when it is created.

| Token | For | May call |
| --- | --- | --- |
| `tc_svc_…` | Service tokens for agents that report messages. The creator is the person of every message. | `POST /token-entries`, `POST /bills`, `POST /bills/csv` and its preview, `POST /reversals`, `POST /reports/saved` |
| `tc_read_…` | Read tokens for reports, spreadsheets and the [MCP server](/reference/api/mcp). | Every `GET`, inside what the creator's grants cover, and `POST /mcp` |
| `tc_org_…` | The organization token for OpenTelemetry. Org managers create it. | `POST /ingest/otlp/v1/logs`, `/v1/traces`, `/v1/metrics` |
| `tc_cli_…` | Kept on the machine by `tc connect`, which signs in with a magic link. | What its person does: report and import their own messages, split them, set their contract, submit, ask for estimates, edit their own rules, and every `GET` |

The web app signs in with a [magic link](/reference/api/sign-in) and keeps a session cookie, `tc_session`. Changes to the organization itself, such as members, tag names, the price table, the cutoff, grants, connections and received records, are made by a person in the web app, not by a token. A call with no token or an unknown one gets `401`; a token that may not make that call gets `403`.

## Errors {#errors}

Every error is JSON with a stable code and a sentence:

```json
{ "error": "forbidden", "message": "plan on feature: KD-1 is needed" }
```

| Status | `error` | When |
| --- | --- | --- |
| `400` | `bad_request` | The body or a parameter cannot be read. The message names the field. |
| `401` | `unauthorized` | No token, an unknown token, or a token whose person left the organization. |
| `403` | `forbidden` | The token may not make this call, or the caller lacks the [capability](/account/roles#capabilities) it needs on that filter. |
| `404` | `not_found` | No such record inside what the caller may view. |
| `409` | `conflict` | The call makes no sense now: an event already reversed, a member limit reached, a price row already in force, no exchange rate for a report's currency. |
| `410` | `gone` | A sign-in link that was used or has expired. |
| `413` | `too_large` | An OpenTelemetry body that is too large. |
| `415` | `unsupported_media_type` | An OpenTelemetry body in another content type or encoding. |
| `429` | `too_many_requests` | Too many sign-in links for one address, or the organization reached its [daily limit](/plans#messages) of messages or of bytes received; nothing of the request was stored. |
| `500` | `internal` | Something went wrong on our side. |
| `503` | `database_unavailable` | The database does not answer. Try again. |

Every answer carries an `x-request-id` header; quote it when you write to support.

## How the API is built {#conventions}

**Every write is an event.** Nothing in the log is edited or deleted. A source record is reversed with `POST /reversals`; an assignment such as a split, a contract assignment, an accept or a grant supersedes the earlier one on the same target. A record sent twice is one record: the same session id and message id, the same `local_id`, the same bill figures for a contract and range. What a usage endpoint received is kept apart from the log, as [received records](/reference/api/messages#received-records), and only those can be deleted. See [Data model](/reference/data-model).

**Every read is as of a date.** Lists and reports take `as_of`, a day as `YYYY-MM-DD` or a moment in RFC 3339. A day means its last moment in UTC, so `as_of=2026-04-05` answers `"as_of": "2026-04-05T23:59:59.999999999Z"`. Left out, it is now. As of the [cutoff](/account/cutoff) gives the frozen figures.

**Dates** are `YYYY-MM-DD` in UTC, both ends of a range included. Moments are RFC 3339, such as `2026-03-18T09:00:00Z`.

**Money** is a decimal string, such as `"580.00"`, never a float. Cost is in US dollars unless a report converts it; a price per million tokens is written in full, such as `"0.3"`. Fractions are decimal strings that add up to 1, such as `"0.6"`. Counts are integers.

**Tags** are a JSON object of tag name to key, such as `{"feature": "KD-1", "client": "acme"}`. One value per tag name.

**Filters** in a query string are comma-separated `name:value` pairs, such as `filter=client:acme,team:web`. An empty filter is the whole organization. A read answers inside the caller's view grants; a filter outside them gets `403`.

**Lists** are newest first and take `limit`; the [event log](/reference/api/reports#events) runs the other way, the latest last. There is no page cursor: narrow with `from`, `to` and `filter`. Tables also come as CSV with `format=csv`.

**Ids** carry their kind: `msg_` a message, `act_` an activity, `bil_` a bill, `est_` an estimate, `tag_` a tag value, `att_` an attribute value, `pat_` a person's attribute, `spl_` a split, `con_` a contract assignment, `acc_` an accept, `shr_` a share, `prc_` a price row of your own, `prm_` a managed price row, `grt_` a grant, `cut_` a cutoff, `sub_` a submit, `apr_` an approval, `rul_` a rule, `rev_` a reversal, `rpt_` a saved report, `rcv_` a received record. Every id but `rcv_` is an event id and can be reversed; managed price rows cannot. Members, tokens and connections have UUIDs.

## OpenAPI and the SDK {#openapi}

`GET /api/v1/openapi.json` is the OpenAPI 3.1 document of the running service, with every endpoint and schema on these pages. Point any client generator at it.

The TypeScript SDK, `@tokencontroller/sdk`, wraps the token entries endpoint with retries and a schema check before sending:

```ts
import { TokenController } from '@tokencontroller/sdk'

const tc = new TokenController({ token: process.env.TOKEN_CONTROLLER_TOKEN })
await tc.tokenEntries.create({
  agent_harness: 'claude-code',
  agent_name: 'review bot',
  provider: 'anthropic',
  model: 'claude-sonnet-4-5',
  session_id: 'S3',
  started_at: '2026-03-18T09:00:00Z',
  ended_at: '2026-03-18T09:40:00Z',
  tokens: { input: 10000000 },
  project: 'shop',
})
```

The examples on these pages use the [worked month](/concepts#month) of Kestrel Digital, March 2026: three contracts (`claude-team`, flat; `anthropic-api` and `openai-api`, metered), four sessions S1 to S4 that cost 950, three bills of 270, 300 and 250, features KD-1 and KD-2 in project Shop for client Acme, and Mara and Tom in team web. Each session is one message, priced at the example organization's own row of 10 US dollars per million input tokens.
