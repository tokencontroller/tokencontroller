---
title: "Telemetry push"
description: "Settings for Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor Enterprise to push usage over OpenTelemetry, and what telemetry can and cannot see."
---

Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor Enterprise can push their usage straight to your organization over OpenTelemetry. Nothing is installed on anyone's machine. Every agent sends to the same ingest URL, `https://app.tokencontroller.com/api/v1/ingest/otlp`, with your organization token (`tc_org_…`, under _Settings_ in the web app).

## Settings by agent {#settings}

| Agent | Where to set it | Ingest URL | Token | Also set |
| --- | --- | --- | --- | --- |
| [Claude Code](#claude-code) | Managed settings, `env` | `OTEL_EXPORTER_OTLP_ENDPOINT` | `OTEL_EXPORTER_OTLP_HEADERS` | `CLAUDE_CODE_ENABLE_TELEMETRY`, `OTEL_LOGS_EXPORTER`, `OTEL_METRICS_INCLUDE_REPOSITORY` |
| [Codex CLI](#codex) | `[otel]` in `config.toml` | `exporter.otlp-http.endpoint`, ending in `/v1/logs` | `headers` | `protocol = "binary"`, `log_user_prompt = false` |
| [Gemini CLI](#gemini-cli) | `GEMINI_TELEMETRY_*` variables, or `telemetry` in `settings.json` | `GEMINI_TELEMETRY_OTLP_ENDPOINT` | `GEMINI_TELEMETRY_OTLP_HEADERS` | `GEMINI_TELEMETRY_ENABLED`, `GEMINI_TELEMETRY_OTLP_PROTOCOL=http` |
| [Copilot CLI](#copilot-cli) | Environment variables | `OTEL_EXPORTER_OTLP_ENDPOINT` | `OTEL_EXPORTER_OTLP_HEADERS` | Nothing; leave message content capture off. |
| [Cursor Enterprise](#cursor) | _Team Settings_ > _OpenTelemetry Export_ in Cursor | Base URL | Auth header | Logs on; conversation content off. |

The ingest endpoint takes OTLP over HTTP, as protobuf or JSON, plain or gzip. It does not take gRPC.

The ingest endpoint never stores prompt text: it removes it from model call events and drops any other event that carries it. Turning prompt logging off in the agent still sends less data.

Only Claude Code sends a cost with each model call. For the other agents Token Controller computes the list value from the model and the [price table](/reference/pricing-tables).

## Claude Code {#claude-code}

An administrator adds this to Claude Code's managed settings:

```json
{
  "env": {
    "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
    "OTEL_METRICS_EXPORTER": "otlp",
    "OTEL_LOGS_EXPORTER": "otlp",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/protobuf",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "https://app.tokencontroller.com/api/v1/ingest/otlp",
    "OTEL_EXPORTER_OTLP_HEADERS": "Authorization=Bearer tc_org_…",
    "OTEL_METRICS_INCLUDE_REPOSITORY": "true"
  }
}
```

The exporter adds `/v1/metrics` and `/v1/logs` to the endpoint itself. Each model call is counted once, from its log event; metrics are accepted and not counted, because they repeat the same calls. `OTEL_METRICS_INCLUDE_REPOSITORY` adds the repository, which places cost on projects.

## Codex CLI {#codex}

In `config.toml` (in `CODEX_HOME`, by default `~/.codex`):

```toml
[otel]
log_user_prompt = false
exporter = { otlp-http = {
  endpoint = "https://app.tokencontroller.com/api/v1/ingest/otlp/v1/logs",
  protocol = "binary",
  headers = { "Authorization" = "Bearer tc_org_…" }
}}
```

Codex sends the URL as written, so it ends in `/v1/logs`. The [CLI](/collect/cli) also reads Codex session files, which adds the branch and lets agent managers review before they submit.

## Gemini CLI {#gemini-cli}

```sh
GEMINI_TELEMETRY_ENABLED=true
GEMINI_TELEMETRY_OTLP_ENDPOINT=https://app.tokencontroller.com/api/v1/ingest/otlp
GEMINI_TELEMETRY_OTLP_PROTOCOL=http
GEMINI_TELEMETRY_OTLP_HEADERS="Authorization=Bearer tc_org_…"
GEMINI_TELEMETRY_LOG_PROMPTS=false
```

The same settings go in `settings.json` under `telemetry`: `enabled`, `otlpEndpoint`, `otlpProtocol`, `otlpHeaders` and `logPrompts`. Gemini CLI sends gRPC unless told otherwise, so `http` is required. `logPrompts` set to `false` is optional and sends less data. Session files are not read.

## Copilot CLI {#copilot-cli}

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=https://app.tokencontroller.com/api/v1/ingest/otlp
OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer tc_org_…"
```

Setting the endpoint switches telemetry on. Copilot CLI reports each model call as a trace span, which Token Controller reads. Leave `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` off to send less data; the calls count either way. Session files are not read.

## Cursor Enterprise {#cursor}

Cursor pushes from its own servers, so an administrator sets it up once in Cursor under _Team Settings_ > _OpenTelemetry Export_:

- **Base URL**: `https://app.tokencontroller.com/api/v1/ingest/otlp`, with no `/v1` at the end; Cursor adds the paths.
- **Auth header**: `Authorization: Bearer tc_org_…`.
- Test the connection, then enable it. Keep logs on: each model call is a log event. Leave conversation content export off.

Cursor uses its own names for the same fields; Token Controller maps them. Session files are not read.

## Tags in telemetry {#tags}

Agents that read `OTEL_RESOURCE_ATTRIBUTES`, such as Claude Code and Copilot CLI, can tag everyone who gets the setting, for example with one managed settings file per team:

```sh
"OTEL_RESOURCE_ATTRIBUTES": "team=web,department=engineering"
```

Attributes whose names are on your organization's tag list become tags on the token entries built from that telemetry. Others are ignored. Telemetry tags are the same for every session on a machine, so they suit team or department, not tickets.

## What telemetry sees, and what it does not {#sees}

Each telemetry event is one model call: tokens, model, person, repository and the IDs of the session and message, plus cost from Claude Code. It does not see the branch, the pull request or a summary. Organizations that only push telemetry get cost on projects, by repository, and nobody fills in a token sheet: the token entries are [automatic](/collect#automatic). Adding the [mod](/collect/claude-code) or the [CLI](/collect/cli) puts cost on tickets, with the person's own review.

When telemetry and a token entry describe the same session, they are joined by session and message ID and counted once.
