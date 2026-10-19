---
title: "Reading reports"
description: "Every report is the cost of a filter over a date range, as of a date: grouping by any tag or attribute name, partitions against overlapping filters, labels, and the finance report."
---

Every report is one number computed one way: the cost of a filter over a date range, as of a date. A filter is any set of tags and attributes, so a report shows cost by person and team, by client, project and feature, or by any tag, such as cost per purpose for `client: acme`.

## The one formula {#formula}

| | |
| --- | --- |
| Cost of a filter over a date range | the sum, over every split whose full tag set contains the filter and whose message falls in the range, of fraction × message cost |

The full tag set is the split's own tags with their defaults (feature brings project, project brings client) and the person's attributes with theirs (team brings department). Nothing else is ever summed. Plans and deviations read this same number; see [Deviation and forecast](/report/deviation).

## Grouping {#grouping}

Group a report by any tag name or attribute name: `project`, `feature`, `client`, `team`, `department`, `person`, or by agent harness, agent name, model or day. For one name, the values partition the total: cost per project adds up to the filter's cost, with one line for splits that have no value. In the [worked month](/concepts#month) every grouping of the same 950 adds up: Mara 700 and Tom 250; Shop 800 and none 150; team web 950.

Filters on different names overlap: `team: web` and `project: shop` can count the same money. Add up lines within one grouping, never across two reports.

## Labels {#labels}

Grouping by label splits cost into direct (a feature), project overhead (a project, no feature) and organization overhead (no project). In the worked month: 700, 100 and 150. Every split keeps its person, so organization overhead is never nobody's.

## As of {#as-of}

Every report carries its as-of date. As of today it includes every event; as of the [cutoff](/account/cutoff) it is frozen and gives the same answer every time. Hand finance a report as of the cutoff.

## The bill {#bill}

A filtered report never reaches the bill: what the bill has that no message explains carries no tags. The [reconciliation](/account/bills#reconciliation) on each bill's own range is where the two meet.

## Views {#views}

Every report can be shown in another currency at the rate of the range's last day, or as each line's share of the filter's cost instead of money. With [approval](/account/submit#approval) on, a report can show approved messages only.

## Summaries {#summary}

Every session carries the agent's summary and its facts, so a number in a report can always be opened down to what the work was.

## Agent harness reports {#agent-harness}

| Grouping | Example question |
| --- | --- |
| Agent harness | What do we spend in Claude Code against Codex? |
| Agent name | What does the review bot cost across all projects? |
| Model | How much goes to the largest model? |

## Saved reports {#saved}

A saved report is a filter, a grouping and a range, kept for the organization. Everyone sees it with the cost their grants cover; its creator or an org manager reverses it.
