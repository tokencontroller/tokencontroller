---
title: "Reports and the event log"
description: "The cost report C(F, w) with its on-a-plan and not-priced-yet lines, the deviation and forecast reports, saved reports, the event log itself and the raw export, with every parameter."
---

Every report is one formula: the cost of a filter over a date range, grouped by any tag or attribute name. Plans, deviations and forecasts read that number and nothing else. Every report answers inside the caller's view grants and carries the `as_of` it used.

| Endpoint | What it returns | Who |
| --- | --- | --- |
| `GET /reports/cost` | C(F, w): the cost of a filter over a range, grouped. The main report. | View on the filter |
| `GET /reports/deviation` | Plan, cost and deviation per value of the grouping, with labels. | View on the filter |
| `GET /reports/forecast` | The cost of the filter in the next range, from the three ranges before it. | View on the filter |
| `GET /reports/saved`, `POST /reports/saved` | The organization's saved reports; save a filter, grouping and range as one. | Anyone; signed in or with a service token to save |
| `GET /events` | The log itself, in order, inside what you may view. | Anyone |
| `GET /exports/raw` | The whole log as a zip of JSON files. | Org manager |

The reconciliation of bills against messages is on [Bills](/reference/api/bills#reconciliation).

## Parameters {#parameters}

The three reports take the same parameters:

| Parameter | Meaning |
| --- | --- |
| `from`, `to` | The date range, `YYYY-MM-DD` in UTC, both days included. Required. For the forecast, the range ahead. |
| `as_of` | The moment up to which the log is read: a day (its last moment) or an RFC 3339 time. Default now. The [cutoff](/account/cutoff) gives the frozen figures. |
| `by` | What to group by: a tag or attribute name such as `feature`, `team` or `contract`, or `person`, `label`, `agent_harness`, `agent_name`, `model` or `day`. Left out, one row for the whole filter. |
| `filter` | Such as `client:acme,team:web`. Left out, the whole organization, inside your view grants. |
| `approved` | `true` counts approved messages only. |
| `currency` | Show money in another currency, such as `EUR`, at the reference rates of the range's last day. Default: the organization's reporting currency. `409` when no rate is known. |
| `view` | `money` (the default) or `share`, each amount as its fraction of the total. |
| `format` | `csv` for the rows as a CSV file instead of JSON. |

## Cost report {#cost}

```http
GET /api/v1/reports/cost?from=2026-03-01&to=2026-03-31&by=feature&as_of=2026-04-05
Authorization: Bearer tc_read_…
```

```json
{
  "as_of": "2026-04-05T23:59:59.999999999Z",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "filter": {},
  "by": "feature",
  "currency": "USD",
  "view": "money",
  "total": "950.00",
  "rows": [
    { "key": null, "amount": "250.00", "share": "0.263158", "messages": 2 },
    { "key": "KD-1", "amount": "580.00", "share": "0.610526", "messages": 2 },
    { "key": "KD-2", "amount": "120.00", "share": "0.126316", "messages": 1 }
  ],
  "count_errors": 0,
  "unknown_models": [],
  "not_priced_yet": { "messages": 0, "tokens": {} },
  "unpriced_kinds": [],
  "on_a_plan": "400.00",
  "metered": "550.00",
  "no_contract": "0.00"
}
```

The rows of one grouping add up to `total`: 250 + 580 + 120 = 950. The row with `key: null` comes first and holds the cost with no value for that name, here Tom's review bot on Shop and his session on nothing. `messages` counts the messages a row has a part of, so S2, split over KD-1 and KD-2, counts in both.

`on_a_plan` is the part of `total` under a flat contract, which a fee pays: S1 under the seat plan `claude-team`. `metered` is the part under metered contracts and `no_contract` the part with no contract yet; the three add up to `total`. With `view=share` they are fractions too.

`count_errors` are messages in the range with no counts. `not_priced_yet` counts the messages whose price is not certain yet, with their tokens added up by kind, such as `{"messages": 3, "tokens": {"input": 41200, "output": 6100}}`, and `unknown_models` names their models; they are not in `total`, and show as their own line, never as zero. `unpriced_kinds` are token kinds with tokens and no price row, such as `reasoning`: they are left out, and the rest of each message is priced. See [Not priced yet](/reference/pricing-tables#unknown).

The same month, other groupings: `by=project` gives shop 800 and null 150; `by=client` acme 800; `by=person` Mara 700, Tom 250; `by=label` direct 700, project_overhead 100, organization_overhead 150; `by=team` web 950; `by=contract` anthropic-api 250, claude-team 400, openai-api 300. A filter narrows before grouping: `filter=feature:KD-1` gives total 580; `filter=team:web&by=feature` the same three rows. See [Reports](/report/reports) and [Checking a number](/report/checking-a-number).

## Deviation report {#deviation}

```http
GET /api/v1/reports/deviation?from=2026-03-01&to=2026-03-31&by=project&as_of=2026-04-05
```

```json
{
  "as_of": "2026-04-05T23:59:59.999999999Z",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "filter": {},
  "by": "project",
  "currency": "USD",
  "rows": [
    { "key": "shop", "filter": { "project": "shop" }, "plan": "232.50", "cost": "800.00", "deviation": "-567.50", "favourable": false, "label": "scope" },
    { "key": null, "filter": {}, "plan": null, "cost": "150.00", "deviation": null, "favourable": null, "label": null }
  ]
}
```

`plan` is the accepted estimate spread over the range by days: Shop's estimate of 750 over 100 days gives 232.50 for March. `deviation` is plan minus cost, positive favourable; `label` is `scope`, `trouble` or `estimate_itself` from the classifier where one is set up. `plan` is `null` where nothing is planned, and on the row without a value. With `by=person`, the plan is the sum of the person's [shares](/reference/api/estimates#shares): Mara, 0.6 of Shop, has plan 139.50. Groupings other than a tag or attribute name or `person` have no plan. See [Deviation and forecast](/report/deviation).

## Forecast report {#forecast}

```http
GET /api/v1/reports/forecast?from=2026-04-01&to=2026-04-30&filter=team:web&as_of=2026-04-05
```

`from` and `to` name the range ahead. The forecast is the cost per day over the three ranges of that length before it, times the days of the range:

```json
{
  "as_of": "2026-04-05T23:59:59.999999999Z",
  "from": "2026-04-01",
  "to": "2026-04-30",
  "filter": { "team": "web" },
  "by": null,
  "currency": "USD",
  "rows": [
    {
      "key": null,
      "filter": { "team": "web" },
      "forecast": "316.67",
      "past": [
        { "from": "2026-01-01", "to": "2026-01-30", "cost": "0.00" },
        { "from": "2026-01-31", "to": "2026-03-01", "cost": "0.00" },
        { "from": "2026-03-02", "to": "2026-03-31", "cost": "950.00" }
      ]
    }
  ]
}
```

Team web's 950 in March over the 90 days before April gives 950 × 30 / 90 = 316.67; Kestrel Digital has no earlier usage. With `by`, there is one row per value that had cost in the past ranges. A forecast is a report, never a record: nothing is stored.

## Saved reports {#saved}

```http
POST /api/v1/reports/saved

{ "name": "Acme by feature", "filter": { "client": "acme" }, "by": "feature", "from": "2026-03-01", "to": "2026-03-31" }
```

`201`:

```json
{
  "id": "rpt_3f0b…",
  "name": "Acme by feature",
  "filter": { "client": "acme" },
  "by": "feature",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "created_by": "nadia@kestrel.example",
  "created_at": "2026-04-05T08:00:00Z"
}
```

`GET /reports/saved` lists the organization's saved reports, oldest first. Remove one with [`POST /reversals`](/reference/api/organization#reversals) and its id.

## The event log {#events}

```http
GET /api/v1/events?kind=split&as_of=2026-03-31&limit=100
```

The log in order, the latest last, inside what you may view: your own events, and those whose figures lie inside your view grants; org managers see all. `kind` keeps one kind, in the words of the docs: `message`, `activity`, `bill`, `estimate`, `tag value`, `attribute value`, `person attribute`, `share`, `price table row`, `split`, `contract assignment`, `accept`, `grant`, `cutoff`, `submit`, `approval`, `rule`, `reversal`. `limit` keeps the latest events, 500 by default, at most 10,000.

```json
[
  {
    "id": "spl_0c3a…",
    "recorded_at": "2026-03-12T16:00:00Z",
    "recorded_by": "mara@kestrel.example",
    "supersedes": null,
    "kind": "split",
    "record": {
      "split": {
        "target": { "session": "S2" },
        "parts": [
          { "weight": 600000, "tags": { "feature": "KD-1" } },
          { "weight": 400000, "tags": { "feature": "KD-2" } }
        ]
      }
    }
  }
]
```

`record` is the event as the accounting core writes it, under its kind, so a reader folds the list back to the same figures the service shows. A split holds weights, and a part's fraction is its weight over their sum. A message's record carries `received`, the id of the [received record](/reference/api/messages#received-records) it was read from. `supersedes` names the older event this one replaces.

## Raw export {#raw-export}

```http
GET /api/v1/exports/raw?as_of=2026-03-31
```

A zip file, `token-controller-export-2026-03-31.zip`, with one JSON file per kind of event under `events/`, such as `events/tag_value.json`, each event as `GET /events` writes it; `price_table.json` with the rows in force; `grants.json`; and `about.json` with the organization's id, the `as_of`, the currency and the cutoff. Received records are not in it. Org managers only. Reading it back as of any date gives the same figures the service shows. See [Raw export](/report/exports#raw-export).
