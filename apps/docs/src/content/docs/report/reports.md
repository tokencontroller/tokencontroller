---
title: "Reading reports"
description: "The reports, what they answer, cost by agent harness and model, outliers, and how summaries and status show in them."
---

A report is a saved [tag filter](/account/tags#filters) with a grouping, so it shows cost from both sides: by person and team, and by client, project and ticket, or by any tag, such as cost per feature for `client: acme`. Every report can be shown in another currency, or in the share view: each line's share of the period instead of money. Each person sees the reports for what their role covers.

![Reports: cost by person for the month, with the filter and grouping above](/screens/10-reports.png)

## The reports {#list}

| Report | Answers |
| --- | --- |
| Your own reports | Any saved tag filter with a grouping, such as cost per team for `client: acme`. |
| [Token budget vs actual](/account/token-budgets#actual) | Which token budgets ran over or under? |
| [Agent harness reports](#agent-harness) | What does each agent harness, agent name and model cost? |
| [Outliers](#outliers) | Which work cost far more or less than usual? |
| [Cost sheet](/account/close#cost-sheet) | What did a closed period cost, per project and ticket, adding up to the bill? |

Reports filtered by different tags overlap: cost per team and cost per client can count the same money. Add up lines within one report, never across reports. Residual has no tags, so a filtered report never reaches the bill total; only the cost sheet does. More in [Checking a number](/report/checking-a-number).

## Agent harness reports {#agent-harness}

| Cut | Example question |
| --- | --- |
| Agent harness | What do we spend in Claude Code against Codex? |
| Agent name | What does the review bot cost across all projects? |
| Model | How much goes to the largest model? |
| Effort and speed | What do the high-effort settings add? |

## Outliers {#outliers}

An outlier is a ticket whose cost is far from your organization's own baseline for similar work. Token Controller compares each ticket with the tickets of the same project in the twelve months before the period, or of the whole organization when the project has fewer than three. It never compares with other customers. The baseline is their median cost. The report lists the tickets furthest from it, high or low: a ticket at twice the baseline is as far as one at half.

An outlier is a question to ask, not an alert: nothing is sent and nothing is stopped. Open its token entries and read the summaries.

## Summaries {#summary}

Every token entry carries the agent's summary, so a number in a report can always be opened down to what the work was.

## Status {#filters}

Submitted token entries count at once. Drafts and rejected token entries do not count. With [approval](/account/cadence#approval) on, reports can show all submitted token entries or approved ones only.
