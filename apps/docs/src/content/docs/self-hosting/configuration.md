---
title: "Configuration"
description: "Environment variables for a self-hosted instance, including the Enterprise plan settings."
---

The service reads its settings from environment variables. It refuses to start when a required one is missing or wrong, and names the variable to fix.

## Variables {#vars}

| Variable | Meaning |
| --- | --- |
| `DATABASE_URL` | The Postgres connection string. Required. |
| `TC_PUBLIC_URL` | The address people and local tools use, starting with `https://` or `http://`. Required. |
| `TC_SECRET_KEY` | A random string of at least 32 characters for signing sessions and tokens, such as from `openssl rand -hex 32`. Required. |
| `TC_SMTP_URL` | The mail server for sign-in links, invites and reminders, such as `smtps://user:password@smtp.example.com:465`. Required for magic links. |
| `TC_SMTP_FROM` | The sender of that mail, such as `Token Controller <tc@example.com>`. Without it, mail comes from `noreply@` and the host of `TC_PUBLIC_URL`. |
| `TC_PRICE_TABLE` | The path of your [price table](/reference/pricing-tables#file) file, a CSV with provider, model, kind, price per million and valid from. The open edition ships without prices, so without this file every message is an unknown model. The file is read at start and when it changes; each new row is an event. See [Price table](#price-table). |
| `TC_RATES_REFRESH` | `on` (the default) or `off`. `off` stops the daily exchange rate refresh, for instances without internet access. Bills in another currency then need their exchange rate. |
| `TC_CLASSIFIER_URL` | A [System 1 classifier](/glossary#system-1-classifier) API as the [estimator](/account/estimates) and for deviation labels, such as OpenAI's Decisions API at `https://api.openai.com/v1` or a model server of your own. Without it, the instance has no estimates from a classifier and no labels; people still type estimates. Needs `TC_CLASSIFIER_MODEL`. A self-hosted classifier learns from that instance's data only. |
| `TC_CLASSIFIER_MODEL` | The classifier model, such as `gpt-6-luna`. Needs `TC_CLASSIFIER_URL`. |
| `TC_CLASSIFIER_KEY` | The API key for `TC_CLASSIFIER_URL`, if it needs one. |

Without a mail server, someone with access to the container can still sign a member in. This prints a sign-in link that works once and for 15 minutes:

```sh
docker compose exec tokencontroller tokencontroller sign-in-link ana@example.com
```

## Price table {#price-table}

The open edition ships without prices. Write the rows you pay in a CSV file (the format is under [Price tables](/reference/pricing-tables#file)) and point `TC_PRICE_TABLE` at it; mount it into the container. An org manager can still add rows and overrides in the web app, and those win over the file. With an Enterprise key the instance fetches the managed table from us, and the file is only for rows of your own, or for the managed table on an instance without internet access.

## Enterprise plan settings {#enterprise}

These switch on the [Enterprise plan](/self-hosting/enterprise) features in the Enterprise image. The open-source image ignores them.

| Variable | Meaning |
| --- | --- |
| `TC_LICENSE_KEY` | The license key that comes with the contract. With it the instance fetches the managed price table from `app.tokencontroller.com` at start and every six hours; allow outbound HTTPS there, or ask us for the table as a file for `TC_PRICE_TABLE`. Without a valid key, the image works like the open-source one. From 30 days before the key's last day of support, org managers see a renewal notice. The features keep working for 30 days after that day. |
| `TC_TELEMETRY_FORWARD_URL` | Your collector's OTLP HTTP endpoint, such as `https://otel.example.com:4318`. Every telemetry batch the service accepts goes there unchanged, before the service drops prompt text. |
| `TC_TELEMETRY_FORWARD_HEADERS` | Headers for your collector, as `name=value,name2=value2`, the same as `OTEL_EXPORTER_OTLP_HEADERS`. |

### Cloud bill connectors {#cloud-bills}

Each cloud bill connector is on when its settings are set. Once a day it fetches last month's bill and this month's so far; changed figures make a new bill for the range and reverse the old one. Clouds still restate the first days after a month ends, so last month's bill is [final](/reference/bill) from the 6th; move the [cutoff](/account/cutoff) after that.

| Variable | Meaning |
| --- | --- |
| `TC_BEDROCK_ACCESS_KEY_ID`, `TC_BEDROCK_SECRET_ACCESS_KEY` | An AWS access key allowed only `ce:GetCostAndUsage`. AWS charges a cent per request. `TC_BEDROCK_SESSION_TOKEN` is for temporary keys. |
| `TC_VERTEX_SERVICE_ACCOUNT`, `TC_VERTEX_BILLING_TABLE` | A Google Cloud service account key (JSON) with BigQuery Job User on its project and BigQuery Data Viewer on the billing export dataset, and the export table as `project.dataset.table`. Google Cloud has no cost API, so the connector reads the [billing export](https://cloud.google.com/billing/docs/how-to/export-data-bigquery). |
| `TC_FOUNDRY_TENANT_ID`, `TC_FOUNDRY_CLIENT_ID`, `TC_FOUNDRY_CLIENT_SECRET`, `TC_FOUNDRY_SCOPE` | An Entra app registration with Cost Management Reader on the scope, and the scope, such as `/subscriptions/<id>`. |
| `TC_BEDROCK_SERVICES`, `TC_VERTEX_SERVICES`, `TC_FOUNDRY_SERVICES` | Optional. Comma-separated parts of service names that count as model usage. Defaults: Bedrock `bedrock`; Vertex `vertex ai`, `claude`, `gemini`; Foundry `foundry`, `azure openai`, `cognitive services`, `claude`, `anthropic`. |
