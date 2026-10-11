---
title: "Reading reports"
description: "The reports, what they answer, and how summaries and status show in them."
---

Reports show cost from both sides: by person and team, and by client, project and ticket. A report is a saved [tag filter](/account/tags#filters) with a grouping, so any tag works, such as cost per feature for `client: acme`. Every report can be shown in another currency, or as shares of the period instead of money. Each person sees the reports for what their role covers.

## The reports {#list}

| Report | Answers |
| --- | --- |
| Your own reports | Any saved tag filter with a grouping, such as cost per team for `client: acme`. |
| [Token budget vs actual](/report/token-budget-vs-actual) | Which token budgets ran over or under? |
| [Agent harness reports](/report/agent-harness) | What does each agent harness, agent name and model cost? |
| [Outliers](/report/outliers) | Which work cost far more or less than usual? |
| [Cost sheet](/report/cost-sheet) | What did a closed period cost, per project and ticket, adding up to the bill? |

Reports filtered by different tags overlap: cost per team and cost per client can count the same money. Add up lines within one report, never across reports. Residual has no tags, so a filtered report never reaches the bill total; only the cost sheet does.

## Summaries {#summary}

Every token entry carries the agent's summary, so a number in a report can always be opened down to what the work was.

## Status {#filters}

Submitted token entries count at once. With approval on, reports can show all of them or approved ones only.
