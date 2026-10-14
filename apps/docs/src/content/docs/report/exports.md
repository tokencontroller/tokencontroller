---
title: "Exports and API access"
description: "CSV from every table, the read-only reporting API for spreadsheets and BI tools, and the raw export of everything."
---

| You want | Use |
| --- | --- |
| One table in a spreadsheet | [CSV](#csv) |
| Live data in Excel, Google Sheets or Power BI | The [reporting API](#reporting-api) |
| Everything your organization has, to take with you | The [raw export](#raw-export) |

## CSV {#csv}

Every table in the web app downloads as CSV: token entries with their tags, the cost sheet, reports. The file holds exactly what the table shows, with your filters. The cost sheet's CSV can add any tag name as a column, such as `team` or `purpose`. Without a service, `tc export --csv` writes all token entries from the [CLI](/collect/cli).

## Reporting API {#reporting-api}

A read-only API for reports, usable as a data source in Excel, Google Sheets and Power BI.

```http
GET https://app.tokencontroller.com/api/v1/reports/cost?by=team&filter=client:acme&period=2026-09
Authorization: Bearer tc_read_…
```

Create a read token in the web app under _Settings_. It sees what its creator's role sees. Every report and its parameters are in the [HTTP API](/reference/api) reference.

## Raw export {#raw-export}

Leaving must be easy. A controller chooses _Raw export_ under _Settings_ and gets a zip of JSON files with every token entry, split, tag, bill, rule, ticket and period.

Formatted exports beyond CSV and the raw export are part of the Enterprise plan.
