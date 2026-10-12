---
title: "Configuration"
description: "Environment variables for a self-hosted instance."
---

The service is configured with environment variables.

## Variables {#vars}

| Variable | Meaning |
| --- | --- |
| `DATABASE_URL` | The Postgres connection string. Required. |
| `TC_PUBLIC_URL` | The address people and local tools use. Required. |
| `TC_SECRET_KEY` | A long random string for signing sessions and tokens. Required. |
| `TC_SMTP_URL` | The mail server for magic links and reminders, such as `smtps://user:password@smtp.example.com:465`. Required for magic links. |
| `TC_SMTP_FROM` | The sender of that mail, such as `Token Controller <tc@example.com>`. Without it, mail comes from `noreply@` and the host of `TC_PUBLIC_URL`. |
| `TC_PRICES_REFRESH` | Set to `off` to stop the daily price table and exchange rate refresh, for instances without internet access. Bills in another currency then need their exchange rate. |
| `TC_EMBEDDINGS_URL` | An OpenAI-compatible embeddings API for [token forecasts](/account/token-budgets#forecasts), such as `https://api.openai.com/v1` or a model server of your own. Without it, the instance does without forecasts. Needs `TC_EMBEDDINGS_MODEL`. |
| `TC_EMBEDDINGS_MODEL` | The embeddings model, such as `text-embedding-3-small`. |
| `TC_EMBEDDINGS_KEY` | The API key for `TC_EMBEDDINGS_URL`, if it needs one. |

Without a mail server, a person with access to the container can still sign someone in: `tokencontroller sign-in-link EMAIL` prints a sign-in link for that member, valid once and for 15 minutes.

## Enterprise plan settings {#enterprise}

These turn on the [Enterprise plan](/self-hosting/enterprise) features in the Enterprise image. The open-source image ignores them.

| Variable | Meaning |
| --- | --- |
| `TC_LICENSE_KEY` | The license key that comes with the contract. Without a valid key, the image works like the open-source one. From 30 days before the key's last day of support, controllers see a renewal notice. The features keep working for 30 days after that day. |
| `TC_TELEMETRY_FORWARD_URL` | Your collector's OTLP HTTP endpoint, such as `https://otel.example.com:4318`. Every telemetry batch the service accepts goes there unchanged, before the service drops prompt text. |
| `TC_TELEMETRY_FORWARD_HEADERS` | Headers for your collector, in the form `name=value,name2=value2`, the same as `OTEL_EXPORTER_OTLP_HEADERS`. |

Each cloud bill connector is on when its settings are set. Once a day it fetches last month's bill and this month's so far. A fetch with changed figures is a new revision of that bill. Clouds still restate the first days after a month ends, so last month's bill is [final](/reference/bill) from the 6th, and the [close](/account/close) of that month waits until then.

| Variable | Meaning |
| --- | --- |
| `TC_BEDROCK_ACCESS_KEY_ID`, `TC_BEDROCK_SECRET_ACCESS_KEY` | An AWS access key allowed only `ce:GetCostAndUsage`. AWS charges a cent per request. `TC_BEDROCK_SESSION_TOKEN` is for temporary keys. |
| `TC_VERTEX_SERVICE_ACCOUNT`, `TC_VERTEX_BILLING_TABLE` | A Google Cloud service account key (JSON) with BigQuery Job User on its project and BigQuery Data Viewer on the billing export dataset, and the export table as `project.dataset.table`. Google Cloud has no cost API, so the connector reads the [billing export](https://cloud.google.com/billing/docs/how-to/export-data-bigquery). |
| `TC_FOUNDRY_TENANT_ID`, `TC_FOUNDRY_CLIENT_ID`, `TC_FOUNDRY_CLIENT_SECRET`, `TC_FOUNDRY_SCOPE` | An Entra app registration with Cost Management Reader on the scope, and the scope, such as `/subscriptions/<id>`. |
| `TC_BEDROCK_SERVICES`, `TC_VERTEX_SERVICES`, `TC_FOUNDRY_SERVICES` | Optional. Comma-separated parts of service names that count as model usage. By default: `bedrock`; `vertex ai`, `claude`, `gemini`; `foundry`, `azure openai`, `cognitive services`, `claude`, `anthropic`. |
