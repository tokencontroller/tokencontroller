---
title: "Checking a number"
description: "Why a total, a report or a message shows a figure you did not expect, and what to do about it."
---

Most unexpected figures have one of a few causes: what the bill has that no message explains, filters that overlap, a missing price or count, or a different as-of date.

## Totals and the bill {#bill}

| You see | Why | What to do |
| --- | --- | --- |
| Messages add up to less than the bill | The rest is what the bill has that no message explains: usage in the provider's own chat app, per-call fees the agent harness does not report. | Nothing to fix. It is a line on the [bill](/account/bills#reconciliation). |
| Messages add up to more than the bill | A message is wrong: a wrong count, a wrong model, a price row that is too high, or a message the provider did not charge. | Find it in the reconciliation and reverse it. |
| A filtered report never reaches the bill total | What no message explains has no tags, so no [filter](/account/tags#filters) matches it. | Read the [reconciliation](/account/bills#reconciliation). It is the only view that equals the bill. |
| A message's cost differs from what the agent reported | The agent's figure is a [fact on the message](/reference/token-entry); the cost is counts × the [price table](/account/price-table). | Compare the price rows with your contract and add an override. |
| An Enterprise plan cost is higher than the bill | The managed table has list prices; your discount is not in it. | Override the rows with your prices from the date they apply. |
| The bill changed after it was pulled | The provider [restated](/account/bills#sources) it. The old bill is reversed and stays visible. | Nothing. |
| A bill differs from the provider's invoice in your reporting currency | A bill keeps its own currency and the [exchange rate of its last day](/account/bills#sources). | Compare in the bill's own currency. |

## Reports {#reports}

| You see | Why | What to do |
| --- | --- | --- |
| Two reports count the same money | Filters on different names, such as team and client, [overlap](/account/tags#filters). | Add up lines within one grouping, never across reports. |
| Two plans together exceed the total | Plans sit on filters, and filters overlap. Shares over the people of a project may add up to more than 1. | Read each plan on its own. |
| A report shows a line without a value | You grouped by a name that not every split [has](/report/reports#grouping). | Add the tag with a [rule](/attribute/rules), or by hand in the [check list](/attribute/review). |
| Figures with no bill | Usage on a seat plan (Claude Team, Max, Pro). The price table gives it a cost, and there is no bill to reconcile. | Use the share view to compare without dollars. |
| Much cost is organization overhead | Those messages matched [no feature and no project](/attribute/rules#nothing). | Fix them in the check list and tick remember. |
| Last month's team totals did not follow a team change | Attributes have a [from date](/account/attributes#messages), so earlier messages keep the old team. | Nothing. That is on purpose. |
| Two reports on the same filter and range differ | Different [as-of dates](/report/reports#as-of). | Compare the as-of dates in the headers. |

## Single messages {#entries}

| You see | Why | What to do |
| --- | --- | --- |
| A message is labelled unknown model | The model has no [price row](/account/price-table#missing) on that day. Its cost is never shown as zero. | Add the row, with a valid-from date on or before the message. |
| A message is labelled count error | It came without token counts by kind. | Have the door send the counts; see [Choosing a door](/collect#message). |
| A session seems to count twice, through telemetry and a session file | It does not: the two are one record by [session id and message id](/collect/telemetry#sees). | Nothing. |
| A session is on a feature it only mentioned | Rules cannot tell a mention from work. | Assign it by hand and tick remember. |
| A session is split equally between two features | It named both and nobody decided. | Split it by hand. |

## Before the cutoff {#cutoff}

| You see | Why | What to do |
| --- | --- | --- |
| A frozen figure did not follow a new price row, rule or default | Figures as of the [cutoff](/account/cutoff) are frozen. Rules run back only since the cutoff. | Read the report as of today, or have an org manager [adjust](/account/cutoff#adjust) the event in before the cutoff. |
| A message arrived for a range before the cutoff | It waits, visible, dated when it arrived. | An org manager adjusts it in or reverses it. |
