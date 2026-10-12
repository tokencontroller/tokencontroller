---
title: "Telemetry push"
description: "Push usage over OpenTelemetry with no install, and what telemetry can and cannot see."
---

Many agents can push their usage straight to your organization over OpenTelemetry. Nothing is installed on anyone's machine.

## Claude Code {#claude-code}

An administrator adds this to Claude Code's managed settings. Find your organization token under _Settings_ in the web app.

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

The exporter adds `/v1/metrics` and `/v1/logs` to the endpoint itself. These settings send numbers and identifiers only. The ingest endpoint drops any event that carries prompt text. Each model call is counted once, from its log event; metrics are accepted and not counted, because they repeat the same calls.

## Tags in telemetry {#tags}

Add `OTEL_RESOURCE_ATTRIBUTES` to the same managed settings to tag everyone who gets them, for example one settings file per team:

```sh
"OTEL_RESOURCE_ATTRIBUTES": "team=web,department=engineering"
```

Attributes whose names are on your organization's tag list become tags on the token entries built from that telemetry. Others are ignored. Telemetry tags are the same for every session on a machine, so they suit team or department, not tickets.

## Other agents {#others}

[Codex CLI](/collect/codex), [Gemini CLI](/collect/gemini-cli), [Copilot CLI](/collect/copilot-cli) and [Cursor Enterprise](/collect/cursor) push to the same ingest URL with the same organization token. Each page shows that agent's settings.

## What telemetry sees, and what it does not {#sees}

Each telemetry event is one model call: tokens, cost, model, person, repository and the IDs of the session and message. It does not see the branch, the pull request or a summary. Organizations that only push telemetry get cost on projects, by repository, and nobody fills in a token sheet. Adding the [mod](/collect/claude-code) or the [CLI](/collect/cli) puts cost on tickets, with the person's own review.

When telemetry and a token entry describe the same session, they are joined by session and message ID and counted once.
