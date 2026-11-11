---
title: "Exports and API access"
description: "CSV of reports, bills and the check, the read-only reporting API for spreadsheets and BI tools with the on-a-plan subtotals and the not-priced line, and the raw export of the whole event log."
---

| You want | Use |
| --- | --- |
| One report or table in a spreadsheet | [CSV](#csv) |
| Live data in Excel, Google Sheets or Power BI | The [reporting API](#reporting-api) |
| Everything your organization has, to take with you | The [raw export](#raw-export) |

## CSV {#csv}

In the web app, the cost report, the deviation report and the price table download as CSV. The file holds the rows the table shows and, for the cost report, the total. Every report and table the API serves answers as CSV too, with `format=csv`:

| Endpoint | Columns |
| --- | --- |
| `GET /reports/cost` | `key`, `amount`, `share`, `messages` |
| `GET /reports/deviation`, `/reports/forecast` | The rows of the report |
| `GET /bills`, `/bills/{id}` | Each bill with the cost of its messages and what no message explains |
| `GET /reconciliation` | The [check](/account/bills#reconciliation) of each bill under its contract: bill, messages, unexplained, fee, extra usage, covered by the plan, and the two lines beside it, `same_provider_no_contract` and `contract_from_fallback` |

A cost report's CSV holds its rows, then the total, the [on a plan](/report/reports#on-a-plan) subtotals (`on a plan`, `metered`, `no contract`) and the [not priced yet](/report/reports#not-priced) line with its number of messages. The reporting API's JSON holds the same figures. Without a service, `tc export --csv` writes every message from the [CLI](/collect/cli).

## Reporting API {#reporting-api}

A read-only API for reports, usable as a data source in Excel, Google Sheets and Power BI.

```http
GET https://app.tokencontroller.com/api/v1/reports/cost?by=team&filter=client:acme&from=2026-03-01&to=2026-03-31&as_of=2026-04-05
Authorization: Bearer tc_read_…
```

Besides `total` and `rows`, the cost report carries:

| Field | Meaning |
| --- | --- |
| `on_a_plan`, `metered`, `no_contract` | The total split by the kind of contract its messages ran under. `on_a_plan` is the part already paid by a flat contract's fee. |
| `not_priced_yet` | The messages not priced yet, with their token counts by kind. Their cost is not in the total. |
| `unknown_models` | The models of those messages, as `provider/model`. |
| `unpriced_kinds` | Token kinds with tokens and no price row, left out while the rest of each message is priced. |
| `count_errors` | Messages with no token counts. |

Each message in `GET /messages` carries `on_a_plan` and its `cost_label`: `priced`, `count_error` or `not_priced_yet`.

Create a read token in the web app under _Settings_. It sees what its creator's grants cover. Every report and its parameters are in the [HTTP API](/reference/api/reports) reference.

## Raw export {#raw-export}

Leaving must be easy. An org manager chooses _Raw export_ under _Settings_ and gets a zip of JSON files: the whole event log, one file per record type, plus the price table rows and the grants. Reading it back as of any date gives the same figures the service shows. Received records are kept outside the log, so they are not in it.
