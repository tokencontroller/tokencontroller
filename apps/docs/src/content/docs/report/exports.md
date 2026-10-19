---
title: "Exports and API access"
description: "CSV of any report and table, the read-only reporting API for spreadsheets and BI tools, and the raw export of the whole event log."
---

| You want | Use |
| --- | --- |
| One report or table in a spreadsheet | [CSV](#csv) |
| Live data in Excel, Google Sheets or Power BI | The [reporting API](#reporting-api) |
| Everything your organization has, to take with you | The [raw export](#raw-export) |

## CSV {#csv}

Every report and table in the web app downloads as CSV: messages with their splits and tags, any report with its grouping, the reconciliation, deviations. The file holds exactly what the table shows, with your filter and as-of date in its header. A report's CSV can add any tag or attribute name as a column. Without a service, `tc export --csv` writes every message from the [CLI](/collect/cli).

## Reporting API {#reporting-api}

A read-only API for reports, usable as a data source in Excel, Google Sheets and Power BI.

```http
GET https://app.tokencontroller.com/api/v1/reports/cost?by=team&filter=client:acme&from=2026-03-01&to=2026-03-31&as_of=2026-04-05
Authorization: Bearer tc_read_…
```

Create a read token in the web app under _Settings_. It sees what its creator's grants cover. Every report and its parameters are in the [HTTP API](/reference/api#reports) reference.

## Raw export {#raw-export}

Leaving must be easy. An org manager chooses _Raw export_ under _Settings_ and gets a zip of JSON files: the whole event log, one file per record type, plus the price table rows and the grants. Reading it back as of any date gives the same figures the service shows.
