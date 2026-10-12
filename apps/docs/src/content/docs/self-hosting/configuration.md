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
| `TC_SMTP_URL` | The mail server for magic links and reminders. Required for magic links. |
| `TC_PRICES_REFRESH` | Set to `off` to stop the daily price table and exchange rate refresh, for instances without internet access. Bills in another currency then need their exchange rate. |
