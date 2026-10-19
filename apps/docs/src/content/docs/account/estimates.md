---
title: "Plans and estimates"
description: "Estimates as distributions from any estimator, accepting one as the plan, shares of a plan per person, and markup."
---

An estimate is what a described piece of work will cost. Accepting one makes it the plan for its tag set. A plan is compared with cost and never stops spending.

## Estimates {#estimates}

An estimate has a description, a tag set, and an amount as a probability distribution in dollars: the chance of each cost band, with its median. Any estimator makes one: the System 1 classifier from the description and the history of every tag value, or a person typing bands. Many estimates may exist for one tag set; each stays a record.

```json
POST /api/v1/estimates
{
  "work": "Refund checkout for a returns portal",
  "tags": { "feature": "KD-1" },
  "markup_percent": 15,
  "context": "Last flow: 702.40 USD, sandbox timeouts"
}
```

`work` is the description. `context` is optional. The answer is the distribution with its median, in dollars; see the [API](/reference/api#estimates). An estimate needs no tag: ask for one, keep it, and tag it later.

The hosted service runs the classifier, trained on the text of all organizations: summaries, estimate descriptions and the tags they got. It answers with probabilities only, so no description or client of another organization can come back out. A self-hosted instance points to an estimator of its own; see [Configuration](/self-hosting/configuration).

## Accept {#accept}

Anyone with [plan](/account/roles#capabilities) on the filter accepts one estimate for a tag set, and that amount is the plan. Accepting another supersedes it; the earlier one stays a record, and a report as of an earlier date still shows it. Accept the median as it is or an estimate you typed; the plan is always an accepted estimate.

| | |
| --- | --- |
| Plan of a filter over a date range | the accepted estimate's amount, spread by days over the estimate's own date range, then cut to the report's range |

An estimate for Shop of 750 over 100 days gives March (31 days) 232.50.

## Shares {#shares}

A share is (person, project, fraction, from, to). It says which part of a project's plan a person carries.

| | |
| --- | --- |
| Plan of a person over a date range | the sum over their shares of fraction × plan of that project over the days the share and the range both cover |

Mara holds 0.6 of Shop from 1 March: her plan for March is 0.6 × 232.50 = 139.50. Shares over the people of a project may add up to more than 1; it is shown, never corrected. A handover is a new share for the new person; the old one ends.

## Markup {#markup}

A markup is an overhead percentage inside the estimate's amount. There is no separate line: an estimate of 500 with 15 percent markup is an estimate of 575.

## Deviation {#deviation}

Plan minus cost, for any filter: a feature, a project, a person, a department. Positive is favourable. The classifier labels each one: scope, trouble, or the estimate itself. See [Deviation and forecast](/report/deviation).

## No caps {#no-caps}

A plan never stops an agent and sends no alert. Your provider offers spend caps if you need them.
