---
title: "Reports and the event log"
description: "The cost report C(F, w), the deviation and forecast reports, saved reports, the event log itself and the raw export, with every parameter."
---

Every report is one formula: the cost of a filter over a date range, grouped by any tag or attribute name. Plans, deviations and forecasts read that number and nothing else. Every report answers inside the caller's view grants and carries the `as_of` it used.

| Endpoint | What it returns | Who |
| --- | --- | --- |
| `GET /reports/cost` | C(F, w): the cost of a filter over a range, grouped. The main report. | View on the filter |
| `GET /reports/deviation` | Plan, cost and deviation per value of the grouping, with labels. | View on the filter |
| `GET /reports/forecast` | The cost of the filter in the next range, from the three ranges before it. | View on the filter |
| `GET /reports/saved`, `POST /reports/saved` | The organization's saved reports; save a filter, grouping and range as one. | Anyone; service tokens may save |
| `GET /events` | The log itself, in order, inside what you may view. | Anyone |
| `GET /exports/raw` | The whole log as a zip of JSON files. | Org manager |

The reconciliation of bills against messages is on [Bills](/reference/api/bills#reconciliation).

## Parameters {#parameters}

The three reports take the same parameters:

| Parameter | Meaning |
| --- | --- |
| `from`, `to` | The date range, `YYYY-MM-DD` in UTC, both days included. Required. For the forecast, the range ahead. |
| `as_of` | The moment up to which the log is read: a day or an RFC 3339 time. Default now. The [cutoff](/account/cutoff) gives the frozen figures. |
| `by` | What to group by: a tag or attribute name such as `feature` or `team`, or `person`, `label`, `agent_harness`, `agent_name`, `model` or `day`. Left out, one row for the whole filter. |
| `filter` | Such as `client:acme,team:web`. Left out, the whole organization, inside your view grants. |
| `approved` | `true` counts approved messages only. |
| `currency` | Show money in another currency, such as `EUR`, at the reference rates. Default: the organization's reporting currency. |
| `view` | `money` (the default) or `share`, each row as its fraction of the total. |
| `format` | `csv` for a CSV file instead of JSON. |

## Cost report {#cost}

```http
GET /api/v1/reports/cost?from=2026-03-01&to=2026-03-31&by=feature&as_of=2026-04-05
Authorization: Bearer tc_read_…
```

```json
{
  "from": "2026-03-01",
  "to": "2026-03-31",
  "as_of": "2026-04-05T23:59:59Z",
  "filter": {},
  "by": "feature",
  "currency": "USD",
  "view": "money",
  "total": "950.00",
  "count_errors": 0,
  "unknown_models": [],
  "rows": [
    { "key": "KD-1", "amount": "580.00", "share": "0.610526", "messages": 2 },
    { "key": "KD-2", "amount": "120.00", "share": "0.126316", "messages": 1 },
    { "key": null, "amount": "250.00", "share": "0.263158", "messages": 2 }
  ]
}
```

The rows of one grouping add up to `total`: 580 + 120 + 250 = 950. `key: null` is the cost with no value for that name, here Tom's review bot on Shop and his session on nothing. `messages` counts the messages a row has a part of, so a split session counts in each of its rows. `count_errors` are messages in the range with no counts; `unknown_models` have no price row; neither is in `total`.

The same month, other groupings: `by=project` gives shop 800 and null 150; `by=client` acme 800; `by=person` Mara 700, Tom 250; `by=label` direct 700, project_overhead 100, organization_overhead 150; `by=team` web 950. A filter narrows before grouping: `filter=feature:KD-1` gives total 580; `filter=team:web&by=feature` the same three rows. See [Reports](/report/reports) and [Checking a number](/report/checking-a-number).

## Deviation report {#deviation}

```http
GET /api/v1/reports/deviation?from=2026-03-01&to=2026-03-31&by=project
```

```json
{
  "from": "2026-03-01",
  "to": "2026-03-31",
  "as_of": "2026-04-05T23:59:59Z",
  "filter": {},
  "by": "project",
  "currency": "USD",
  "rows": [
    { "key": "shop", "filter": { "project": "shop" }, "plan": "232.50", "cost": "800.00", "deviation": "-567.50", "favourable": false, "label": "scope" },
    { "key": null, "filter": {}, "plan": null, "cost": "150.00", "deviation": null, "favourable": null, "label": null }
  ]
}
```

`plan` is the accepted estimate spread over the range by days: Shop's estimate of 750 over 100 days gives 232.50 for March. `deviation` is plan minus cost, positive favourable; `label` is `scope`, `trouble` or `estimate_itself` from the classifier where one is set up. `plan` is `null` where nothing is planned. With `by=person`, the plan is the sum of the person's [shares](/reference/api/estimates#shares): Mara, 0.6 of Shop, has plan 139.50. Groupings other than a tag or attribute name or `person` have no plan. See [Deviation and forecast](/report/deviation).

## Forecast report {#forecast}

```http
GET /api/v1/reports/forecast?from=2026-04-01&to=2026-04-30&filter=team:web
```

`from` and `to` name the range ahead. The forecast is the cost per day over the three ranges of that length before it, times the days of the range:

```json
{
  "from": "2026-04-01",
  "to": "2026-04-30",
  "as_of": "2026-03-31T23:59:59Z",
  "filter": { "team": "web" },
  "by": null,
  "currency": "USD",
  "rows": [
    {
      "key": null,
      "filter": { "team": "web" },
      "forecast": "930.00",
      "past": [
        { "from": "2026-01-01", "to": "2026-01-30", "cost": "870.00" },
        { "from": "2026-01-31", "to": "2026-03-01", "cost": "920.00" },
        { "from": "2026-03-02", "to": "2026-03-31", "cost": "1000.00" }
      ]
    }
  ]
}
```

A forecast is a report, never a record: nothing is stored.

## Saved reports {#saved}

```http
POST /api/v1/reports/saved

{ "name": "Acme by feature", "filter": { "client": "acme" }, "by": "feature", "from": "2026-03-01", "to": "2026-03-31" }
```

`201` with the report: `id` (`rpt_…`), `name`, `filter`, `by`, `from`, `to`, `created_by` and `created_at`. `GET /reports/saved` lists the organization's saved reports. Remove one with `POST /reversals` and its id.

## The event log {#events}

```http
GET /api/v1/events?kind=split&as_of=2026-03-31&limit=100
```

The log in order, the latest last, inside what you may view. `kind` keeps one kind, in the words of the docs: `message`, `bill`, `estimate`, `tag value`, `attribute value`, `share`, `price row`, `split`, `accept`, `grant`, `cutoff`, `submit`, `approval`, `rule`, `reversal`. `limit` defaults to 500.

```json
[
  {
    "id": "spl_0c3a…",
    "kind": "split",
    "recorded_at": "2026-03-10T14:45:00Z",
    "recorded_by": "mara@kestrel.example",
    "supersedes": "spl_9b12…",
    "record": { "kind": "split", "session_id": "S2", "message_id": null, "parts": [ { "fraction": "0.6", "tags": { "feature": "KD-1" } }, { "fraction": "0.4", "tags": { "feature": "KD-2" } } ] }
  }
]
```

`record` is the event as the accounting core writes it, so a reader folds the list back to the same figures the service shows. `supersedes` names the older event this one replaces.

## Raw export {#raw-export}

```http
GET /api/v1/exports/raw?as_of=2026-03-31
```

A zip of JSON files, one per kind of record, plus the price table rows in force and the grants. Org managers only. Reading it back as of any date gives the same figures the service shows. See [Raw export](/report/exports#raw-export).
