---
title: "Telemetry push"
description: "Settings for Claude Code, Codex CLI, Gemini CLI, Copilot CLI and Cursor Enterprise to push every model call over OpenTelemetry, and what telemetry can and cannot see."
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

Every event must carry the token counts by kind: input, output, cache read and cache write. Cost comes from the [price table](/account/price-table) for every agent; the cost figure Claude Code sends along is kept as a fact and never shown as the cost.

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

The exporter adds `/v1/metrics` and `/v1/logs` to the endpoint itself. Each model call is one message, from its log event; metrics are accepted and not counted, because they repeat the same calls. `OTEL_METRICS_INCLUDE_REPOSITORY` adds the repository, a signal that a rule turns into the project.

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

Codex sends the URL as written, so it ends in `/v1/logs`. The [CLI](/collect/cli) also reads Codex session files, which adds the branch and lets people check before they submit.

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

## Resource attributes are signals {#tags}

Agents that read `OTEL_RESOURCE_ATTRIBUTES`, such as Claude Code and Copilot CLI, can send a value with every message from everyone who gets the setting, for example with one managed settings file per team:

```sh
"OTEL_RESOURCE_ATTRIBUTES": "purpose=product"
```

A resource attribute is a [signal](/attribute/rules#signals), not a tag: a rule such as "resource attribute `purpose` is tag `purpose` with that key" turns it into a tag value. A person's team and department come from their [attributes](/account/attributes), not from telemetry. Resource attributes are the same for every session on a machine, so they suit a purpose or an environment, not features.

## What telemetry sees, and what it does not {#sees}

Each telemetry event is one message: token counts by kind, model, person, repository and the ids of the session and message. It does not see the branch, the pull request or a summary. Organizations that only push telemetry get cost on projects, by repository, and nobody submits anything. Adding the [mod](/collect/claude-code) or the [CLI](/collect/cli) puts cost on features, with the person's own check.

When telemetry and a session file describe the same message, by session id and message id, they are one record, counted once. For Claude Code that message id is the API request id (`req_…`): telemetry sends it with every call, and the CLI submits it from the session file.
