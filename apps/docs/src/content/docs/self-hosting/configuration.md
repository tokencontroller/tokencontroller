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

## Support and newsletter settings {#customer-relations}

The hosted service uses these to run its own support inbox, site assistant, newsletter and onboarding emails. Each part is off until its settings are set, and a self-hosted instance leaves them all unset.

| Variable | Meaning |
| --- | --- |
| `TC_OPERATOR_EMAILS` | Comma-separated addresses of the people who run the instance. Signed in with one of them, a member sees the admin area: support conversations, site assistant questions, the newsletter, feedback reports and a record of each organization. They get a notice with a link when a support conversation or a feedback report needs a person. Without it, each organization's controllers read its feedback reports. |
| `TC_INBOUND_MAIL_SECRET` | The secret incoming support mail is signed with. Turns on the support inbox. Needs `TC_SUPPORT_FROM`. |
| `TC_SUPPORT_FROM` | The sender of support answers, such as `Support <support@example.com>`. Turns on support inside the app; the answer goes to the member's address. |
| `TC_ANTHROPIC_API_KEY` | A Claude API key. Support answers and the site assistant answer from these docs with it. Without it, every support message goes to the operators. |
| `TC_SITE_URL` | The public website, such as `https://example.com`. Its pages, also under `www.`, may use the site assistant, the contact form and the newsletter sign-up. |
| `TC_RESEND_API_KEY` | A Resend API key. Turns on the newsletter. Needs `TC_NEWSLETTER_FROM`. |
| `TC_NEWSLETTER_FROM` | The sender of the newsletter, such as `News <news@news.example.com>`. |
| `TC_ONBOARDING_EMAILS` | Set to `on` to email a new organization's controllers when it has not taken its next first step three days after the last one: first token entry, first token sheet, first bill, first close. One email per step, none after the first close or 30 days without activity. Needs `TC_SMTP_URL`. |

The service deletes site assistant questions, the raw copy of support mail and feedback screenshots after 90 days, a support conversation two years after its last message, and a feedback report two years after its last change. A message's text stays with its conversation until then.

## Billing settings {#billing}

The hosted service bills the Team plan through Stripe with these two. Billing is off unless both are set, and a self-hosted instance leaves them unset.

| Variable | Meaning |
| --- | --- |
| `TC_STRIPE_SECRET_KEY` | The Stripe API key the service calls Stripe with. |
| `TC_STRIPE_WEBHOOK_SECRET` | The signing secret of the Stripe webhook endpoint, `/api/v1/billing/stripe-webhook`. |

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
