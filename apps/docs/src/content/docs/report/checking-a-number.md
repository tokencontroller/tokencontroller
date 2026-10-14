---
title: "Checking a number"
description: "Why a total, a report or a token entry shows a figure you did not expect, and what to do about it."
---

Most unexpected figures have one of a few causes: residual, filters that overlap, estimates on seat plans, or a status that does not count yet.

## Totals and the bill {#bill}

| You see | Why | What to do |
| --- | --- | --- |
| Token entries add up to less than the bill | The rest is [residual](/account/bills#reconciliation): usage in the provider's own chat app, per-call fees the agent harness does not report, or a restated bill. | Nothing to fix. Residual is always shown as its own line. |
| A filtered report never reaches the bill total | Residual has no tags, so no [filter](/account/tags#filters) matches it. | Use the [cost sheet](/account/close#cost-sheet). It is the only view that adds up to the bill. |
| A token entry's value differs from what was charged | Its list value is the agent's reported cost, or its token counts times the price table. A [billed value](/account/bills#values) exists only where a bill or billed telemetry says what was charged. | Read the label on the figure: billed, list-price estimate, or unknown model. |
| An Enterprise plan list value is higher than the bill | List value is [before discount](/account/bills#values). The discount is its own line on the bill. | Nothing. |
| The bill changed after it was pulled | The provider [restated](/account/bills#sources) it. Each bill keeps its revisions. | Nothing. Any difference shows as residual. |
| A bill differs from the provider's invoice in your reporting currency | A bill in another currency keeps its original amount and is converted at the [exchange rate of its date](/account/bills#sources). | Compare in the bill's own currency. |
| You cannot close the period yet | The bill is not [final](/account/bills#what). [Anthropic Enterprise](/account/bills#anthropic-enterprise) figures are final about 30 days after the period ends. With [approval](/account/cadence#approval) on, an unapproved token entry also blocks the close. | Wait for the bill, and have the open token entries approved. |

## Reports {#reports}

| You see | Why | What to do |
| --- | --- | --- |
| Two reports count the same money | Reports filtered by different tags, such as cost per team and cost per client, [overlap](/account/tags#filters). | Add up lines within one report, never across reports. |
| Two token budgets together exceed the total | [Token budgets](/account/token-budgets#budgets) can overlap, too. | Read each token budget on its own; never add them up. |
| A report shows a line without a value | You grouped by a tag that not every token entry [has](/account/tags#filters). | Add the tag with a [rule](/attribute/rules), or by hand in the [review list](/attribute/review). |
| Figures marked as an estimate, with no bill | Usage on a seat plan (Claude Team, Max, Pro) is [tokens at API rates](/account/bills#values), since the seat is a flat fee. | Switch the report to the share view to compare usage without dollars. |
| Reports show more than the cost sheet | Seat plan usage counts in reports but [stays off the cost sheet](/account/bills). With [approval](/account/cadence#approval) on, reports count submitted cost and the cost sheet approved cost only. | With approval on, filter the report to approved token entries only. |
| Much cost is organization overhead | Those token entries found [no ticket and no project](/attribute/rules#nothing). | Fix them in the review list, and turn repeated fixes into rules. |
| Last month's team totals did not follow a team change | Inherited tags are [copied when a token entry arrives](/account/tags#sources), so earlier entries keep the old team. | Nothing. That is on purpose. |

## Single token entries {#entries}

| You see | Why | What to do |
| --- | --- | --- |
| A token entry is missing from reports | It is a draft and was never [submitted](/attribute/review#submit), or a people manager [rejected](/account/cadence#reject) it. | Submit it, or fix it and submit it again. |
| A token entry is labelled unknown model | The model is missing from the [price table](/reference/pricing-tables#unknown), so there is no list value. Its cost is never shown as zero. | It gets a list value once the price table knows the model. |
| A session seems to count twice, through telemetry and on a token sheet | It does not: the two are [joined by session and message ID](/account/cadence#token-sheet) and counted once. | Nothing. |
| A token entry is on a ticket it only mentioned | Rules cannot tell a mention from work. | Place it by hand, or set up the [classifier](/attribute/classifier). |

## Closed periods {#closed}

| You see | Why | What to do |
| --- | --- | --- |
| A closed period did not follow a price change, a new rule or a changed tag map | A period keeps the [price table and tag map](/account/close#after) it was closed with. Rules only run again on [open periods](/attribute/rules#changes). | Correct it with an [adjustment](/account/close#adjustments). |
| A closed period gained a line marked late | A token sheet submitted after the close becomes an [adjustment](/account/cadence#late) to that period, marked late. | Nothing. Adjustments are their own lines. |
