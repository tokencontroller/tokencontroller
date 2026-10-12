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
| `TC_PRICES_REFRESH` | Set to `off` to stop the daily price table refresh, for instances without internet access. |

Without a mail server, a person with access to the container can still sign someone in: `tokencontroller sign-in-link EMAIL` prints a sign-in link for that member, valid once and for 15 minutes.

## Support and newsletter settings {#customer-relations}

The hosted service uses these to run its own support inbox, site assistant and newsletter. Each part is off until its settings are set, and a self-hosted instance leaves them all unset.

| Variable | Meaning |
| --- | --- |
| `TC_OPERATOR_EMAILS` | Comma-separated addresses of the people who run the instance. They get a notice with a link when a support conversation needs a person. |
| `TC_INBOUND_MAIL_SECRET` | The secret incoming support mail is signed with. Turns on the support inbox. Needs `TC_SUPPORT_FROM`. |
| `TC_SUPPORT_FROM` | The sender of support answers, such as `Support <support@example.com>`. |
| `TC_ANTHROPIC_API_KEY` | A Claude API key. Support answers and the site assistant answer from these docs with it. Without it, every support message goes to the operators. |
| `TC_SITE_URL` | The public website, such as `https://example.com`. Its pages, also under `www.`, may use the site assistant, the contact form and the newsletter sign-up. |
| `TC_RESEND_API_KEY` | A Resend API key. Turns on the newsletter. Needs `TC_NEWSLETTER_FROM`. |
| `TC_NEWSLETTER_FROM` | The sender of the newsletter, such as `News <news@news.example.com>`. |

The service deletes site assistant questions and the raw copy of support mail after 90 days, and a support conversation two years after its last message. A message's text stays with its conversation until then.

## Billing settings {#billing}

The hosted service bills the Team plan through Stripe with these two. Billing is off unless both are set, and a self-hosted instance leaves them unset.

| Variable | Meaning |
| --- | --- |
| `TC_STRIPE_SECRET_KEY` | The Stripe API key the service calls Stripe with. |
| `TC_STRIPE_WEBHOOK_SECRET` | The signing secret of the Stripe webhook endpoint, `/api/v1/billing/stripe-webhook`. |
