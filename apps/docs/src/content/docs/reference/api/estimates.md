---
title: "Estimates and plans"
description: "Ask what described work will cost, read an estimate with its cost and deviation, accept one as the plan, and set a person's share of a project."
---

An estimate is what a described piece of work will cost: a probability distribution in US dollars with its median. Any token can ask. Accepting an estimate makes it the plan for its tag set; shares give each person their part of a project's plan.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /estimates` | Ask any estimator for an estimate, or type one. | Any signed-in person; plan on the tag set when one is given |
| `GET /estimates` | The estimates you may view, newest first, each with whether it is the plan and the cost so far. | View on the tag set |
| `GET /estimates/{id}` | One estimate with the cost of its tag set and the labelled deviation. | View on the tag set |
| `POST /estimates/{id}/accept` | Make an estimate the plan for its tag set. | Plan on the tag set, in the web app |
| `GET /shares` | The shares in force whose project you may view. | View on the project |
| `POST /shares` | A person's share of a project's plan between two dates. | Plan on the project, in the web app |

## Ask for an estimate {#estimates}

```http
POST /api/v1/estimates
Authorization: Bearer tc_cli_…

{
  "work": "Refund checkout for a returns portal",
  "tags": { "feature": "KD-1" },
  "from": "2026-03-01",
  "to": "2026-03-31",
  "context": "Last flow: 702.40 USD, sandbox timeouts"
}
```

| Field | Meaning |
| --- | --- |
| `work` | The description: a prompt, a spec, a feature or acceptance criteria. Required. |
| `tags` | The tag set the estimate is for. Optional: ask without one, keep it, and tag it later. |
| `from`, `to` | The work's date range. Default: today, and 30 days later. The plan is spread over these days. |
| `markup_percent` | Overhead inside the amount, such as `15`. Optional. |
| `context` | Anything that helps the estimator, such as the summaries and cost of earlier work. Optional. |
| `median`, `distribution` | Type the estimate yourself instead of asking the classifier: the median in US dollars, and optionally the bands, which add up to 1. |

Each estimate is billed once, whatever the length of `work` and `context`, so send all the context you have. Without `median`, the hosted service's [System 1 classifier](/account/estimates#estimates) answers from the description and the history of every tag value. A self-hosted instance without an estimator answers `409`: send a `median`.

`201 Created`:

```json
{
  "id": "est_4k2q9w",
  "work": "Refund checkout for a returns portal",
  "tags": { "feature": "KD-1" },
  "from": "2026-03-01",
  "to": "2026-03-31",
  "estimator": "classifier",
  "currency": "USD",
  "median": "500.00",
  "markup_percent": 0,
  "amount": "500.00",
  "distribution": [
    { "up_to": "250.00", "p": "0.04" },
    { "up_to": "500.00", "p": "0.46" },
    { "up_to": "750.00", "p": "0.33" },
    { "up_to": "1000.00", "p": "0.12" },
    { "up_to": null, "p": "0.05" }
  ],
  "accepted": false,
  "cost": "580.00",
  "deviation": null,
  "deviation_label": null,
  "recorded_at": "2026-02-27T10:04:00Z"
}
```

`median` is the estimate; `amount` is the plan amount, the median with the markup inside: with `markup_percent: 15` it would be `575.00`. Each band of `distribution` is the chance `p` that the cost ends at or below `up_to`; the last band has no upper end. `estimator` is `classifier`, or the email of the person who typed it. `cost` is the cost of the tag set over the estimate's range so far, where the tag set has one.

## Read estimates {#read}

`GET /estimates/{id}` returns the same shape with today's figures. Once accepted, `deviation` is plan minus cost, positive favourable, and `deviation_label` is `scope`, `trouble` or `estimate_itself` from the classifier, as on [Deviation and forecast](/report/deviation#labels). For KD-1 in the worked month: amount 500, cost 580, deviation `"-80.00"`, label `scope`.

`GET /estimates?as_of=2026-04-05` lists every estimate you may view, newest first, in the same shape. Many estimates may exist for one tag set; `accepted` marks the plan.

## Accept {#accept}

```http
POST /api/v1/estimates/est_4k2q9w/accept
```

`201` with `id`, the accept event's id (`acc_…`), and `estimate`, the estimate as above with `accepted: true`. Accepting another estimate for the same tag set later supersedes this one; a report as of an earlier date still shows it. An estimate with no tags cannot be accepted (`400`). Needs plan on the tag set. See [Accept](/account/estimates#accept).

## Shares {#shares}

A share is a person's fraction of a project's plan between two dates. Shares over the people of a project may add up to more than 1; the sum is shown, never corrected. A handover is a new share for the new person.

```http
POST /api/v1/shares

{ "person": "mara@kestrel.example", "project": "shop", "fraction": "0.6", "from": "2026-03-01" }
```

`to` is optional; left out, the share is open-ended. `201`:

```json
{
  "id": "shr_2b7a…",
  "person": "mara@kestrel.example",
  "project": "shop",
  "fraction": "0.6",
  "from": "2026-03-01",
  "to": null,
  "over": null
}
```

`over` is the sum of the project's shares on the share's first day where it is above 1, such as `"1.2"`. `GET /shares?as_of=2026-03-31` lists the shares in force whose project you may view. A person's plan is the sum of their shares times the plans of those projects; it appears in the [deviation report](/reference/api/reports#deviation) grouped by person. See [Shares](/account/estimates#shares).
