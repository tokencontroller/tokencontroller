---
title: "Reporting API"
description: "A read-only API for spreadsheets and BI tools."
---

A read-only API for reports, usable as a data source in Excel, Google Sheets and Power BI.

## Use it {#use}

```http
GET https://app.tokencontroller.com/api/v1/reports/cost?by=team&filter=client:acme&period=2026-09
Authorization: Bearer tc_read_…
```

Create a read token in the web app under _Settings_. It sees what its creator's role sees. See the [HTTP API](/reference/api) for every report.
