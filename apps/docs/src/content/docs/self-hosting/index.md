---
title: "Self-hosting"
description: "What a self-hosted Token Controller runs, and how to start."
---

Self-hosted Token Controller is the same product as the hosted Team plan, from the same open code, with every Team plan feature. The hosted service adds our billing, support and feedback. You need one container and one Postgres database.

## What you run {#stack}

| Part | What it is |
| --- | --- |
| The service | One program, written in Rust, in one Docker image. It serves the HTTP API, receives telemetry over OpenTelemetry (HTTP and protobuf), runs background jobs such as bill pulls, ticket sync and price updates, and serves the web app. |
| Postgres | All data lives in one Postgres database. See [Postgres](/self-hosting/postgres). |
| Email | Any SMTP server, for magic links and late token sheet reminders. |

Sign-in runs inside the service: magic link by email, and OIDC on the Enterprise plan. There is no second program to run.

## Getting started {#steps}

<ol class="steps">
  <li><b>Start Postgres and the service.</b><p>See <a href="/self-hosting/docker">Docker</a>.</p></li>
  <li><b>Set your public URL and email.</b><p>See <a href="/self-hosting/configuration">Configuration</a>.</p></li>
  <li><b>Point the local tools at it.</b><p><code>tc connect https://tc.example.com</code>, and the telemetry endpoint at <code>https://tc.example.com/api/v1/ingest/otlp</code>.</p></li>
</ol>

## Support {#support}

Community support on GitHub. With a support contract, a self-hosted instance becomes an [Enterprise plan instance](/self-hosting/enterprise). The service is AGPL-3.0; see [Open source and licenses](/reference/open-source).
