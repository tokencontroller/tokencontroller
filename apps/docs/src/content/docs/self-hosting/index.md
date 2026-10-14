---
title: "Run it yourself"
description: "Run Token Controller on your own servers: one Docker image, one Postgres database, upgrades and backups."
---

Self-hosted Token Controller is the same product as the hosted Team plan, from the same open code, with every Team plan feature. The hosted service adds only our billing, support and feedback.

## What you run {#stack}

| Part | What it is |
| --- | --- |
| The service | One program, written in Rust, in one Docker image. It serves the HTTP API and the web app, receives telemetry over OpenTelemetry (HTTP and protobuf), and runs the background jobs, such as bill pulls, ticket sync and price updates. Sign-in runs inside it too: magic link by email, and OIDC on the [Enterprise plan](/self-hosting/enterprise). |
| Postgres | All data lives in one Postgres database. See [Postgres](#postgres). |
| Email | Any SMTP server, for sign-in links, invites and late token sheet reminders. |

## Getting started {#steps}

<ol class="steps">
  <li><b>Start Postgres and the service.</b><p>See <a href="#docker">Docker</a> below.</p></li>
  <li><b>Set your public URL and email.</b><p>See <a href="/self-hosting/configuration">Configuration</a>.</p></li>
  <li><b>Point the local tools at it.</b><p><code>tc connect https://tc.example.com</code>, and the telemetry endpoint at <code>https://tc.example.com/api/v1/ingest/otlp</code>.</p></li>
</ol>

## Docker {#docker}

```yaml
# compose.yaml
services:
  tokencontroller:
    image: ghcr.io/tokencontroller/tokencontroller:1.0
    ports: ["8080:8080"]
    environment:
      DATABASE_URL: postgres://tc:secret@db/tc
      TC_PUBLIC_URL: https://tc.example.com
      TC_SECRET_KEY: change-me # at least 32 random characters, such as from openssl rand -hex 32
      TC_SMTP_URL: smtp://user:pass@mail.example.com:587
    depends_on: [db]
  db:
    image: postgres:17
    environment:
      POSTGRES_USER: tc
      POSTGRES_PASSWORD: secret
    volumes: ["pgdata:/var/lib/postgresql/data"]
volumes:
  pgdata:
```

```sh
docker compose up -d
```

The service listens on port 8080; put a reverse proxy with TLS in front of it. The person who signs up creates the organization and becomes its controller. Every variable is listed under [Configuration](/self-hosting/configuration).

## Postgres {#postgres}

Postgres 15 or newer. A managed database from your cloud works as well as your own. Token entries are small, because no transcripts or prompts are stored: a 150-member organization fits in a small database instance.

## Upgrades and backups {#upgrades}

```sh
docker compose pull
docker compose up -d
```

The service updates the database schema when it starts. Read the [changelog](/changelog) before a new major version.

Only the Postgres database holds state, so backing it up backs up everything. The [raw export](/report/exports#raw-export) is a second copy in plain JSON.

## Support {#support}

Community support on GitHub. With a support contract, a self-hosted instance becomes an [Enterprise plan instance](/self-hosting/enterprise). The service is AGPL-3.0; see [Open source and licenses](/reference/open-source).
