---
title: "Checking a number"
description: "How to walk the check of a bill under its contract, and why a total, a report or a message shows a figure you did not expect."
---

A figure may look wrong for one of a few reasons: what the bill has that no message explains, messages outside the bill's contract, filters that overlap, a missing price or count, or a different as-of date. The check of each bill is the place to start.

## Walking the check {#check}

Each bill is checked on its own date range against the messages under its contract. In the [worked month](/concepts#month), the _Bills_ screen shows these three checks:

| | `anthropic-api`, metered | `openai-api`, metered | `claude-team`, flat |
| --- | ---: | ---: | ---: |
| Bill | 270 | 300 | 250 |
| Cost of messages | 250 | 300 | |
| No message explains | 20 | 0 | |
| Fee | | | 200 |
| Extra usage | | | 50 |
| Covered by the plan | | | 350 |
| Same provider, no contract | 0 | 0 | 0 |
| Contract only from the provider | 0 | 300 | 0 |

<ol class="steps">
  <li><b>Read the main figures.</b><p>Metered: the bill should equal the cost of its messages plus what no message explains, such as usage in the provider's own chat app. Here 250 + 20 = 270, shown as a gap, and 300 + 0 = 300, shown as balanced. Flat: the bill is the fee plus extra usage, 200 + 50 = 250, and covered by the plan is the cost of its messages minus the extra usage, 400 − 50 = 350. Without a known fee, the check says the fee is not known.</p></li>
  <li><b>Read "Same provider, no contract".</b><p>Messages of the bill's provider in its range that have no contract yet. Some of them may belong to this bill. Answer the <a href="/attribute/review#contract">contract question</a> in the review list, or add a <a href="/attribute/rules">rule</a>, and the check follows.</p></li>
  <li><b>Read "Contract only from the provider".</b><p>The part of the messages that got this contract only because the provider has one contract with you. In the worked month, all 300 of OpenAI. That is right as long as you have only one contract there. If some of those agents ran under another contract you have not recorded, such as a seat plan, add its bill. The provider then has two contracts, the fallback stops, and those messages show under "Same provider, no contract" until a rule or your answer places them.</p></li>
  <li><b>Look for count errors and models not priced yet.</b><p>They show under the check. Their cost is not in the cost of messages, so it appears as part of what no message explains until you fix it.</p></li>
</ol>

The formulas are on the [data model](/reference/data-model#formulas) page.

## Totals and the bill {#bill}

| You see | Why | What to do |
| --- | --- | --- |
| Messages add up to less than a metered bill | The rest is what the bill has that no message explains: usage in the provider's own chat app, per-call fees the agent harness does not report, or messages of the provider with no contract yet. | Read the two lines beside the [check](#check). What is left is a line on the [bill](/account/bills#reconciliation). |
| Messages add up to more than a metered bill | A message is wrong: a wrong count, a wrong model, a price row that is too high, a message under the wrong contract, or one the provider did not charge. The check says a message is wrong. | Find it and reverse it, or put it under its contract. |
| A filtered report never reaches the bill total | What no message explains has no tags, so no [filter](/account/tags#filters) matches it. | Read the [check](/account/bills#reconciliation). It is the only view that equals the bill. |
| All reports together are higher than all bills | Reports show cost at the price table. Under a flat contract the fee paid most of it, and that cost is marked [on a plan](/report/reports#on-a-plan). | Read covered by the plan on the flat contract's check. |
| A message's cost differs from what the agent reported | The agent's figure is a [fact on the message](/reference/token-entry); the cost is counts × the [price table](/account/price-table). | Compare the price rows with your contract. |
| Cost is higher than a metered bill with a discount | The managed table has list prices; your discount is not in it. | Add the contract's own [price rows](/reference/pricing-tables#which) from the date they apply. |
| The bill changed after it was pulled | The provider [restated](/account/bills#sources) it. The old bill is reversed and stays visible. | Nothing. |
| A bill differs from the provider's invoice in your reporting currency | A bill keeps its own currency and the [exchange rate of its last day](/account/bills#sources). | Compare in the bill's own currency. |

## Reports {#reports}

| You see | Why | What to do |
| --- | --- | --- |
| Two reports count the same money | Filters on different names, such as team and client, [overlap](/account/tags#filters). | Add up lines within one grouping, never across reports. |
| Two plans together exceed the total | Plans sit on filters, and filters overlap. Shares over the people of a project may add up to more than 1. | Read each plan on its own. |
| A report shows a line without a value | You grouped by a name that not every split [has](/report/reports#grouping). | Add the tag with a [rule](/attribute/rules), or by hand in the [review list](/attribute/review). |
| Cost marked on a plan | Its messages ran under a flat contract, such as a seat plan, whose fee already paid for them. Reports still show their cost at the price table. | Nothing. What the plan was worth is covered by the plan on its bill's check. |
| Cost with no contract | No step gave those messages a contract. | Answer the [contract question](/attribute/review#contract) in the review list. |
| Much cost is organization overhead | Those messages matched [no feature and no project](/attribute/rules#nothing). | Fix them in the review list and tick remember. |
| Last month's team totals did not follow a team change | Attributes have a [from date](/account/attributes#messages), so earlier messages keep the old team. | Nothing. That is on purpose. |
| Two reports on the same filter and range differ | Different [as-of dates](/report/reports#as-of). | Compare the as-of dates in the headers. |

## Single messages {#entries}

| You see | Why | What to do |
| --- | --- | --- |
| A message is not priced yet | The price table has no [row](/account/price-table#missing) for its model under its provider on that day, or it came without a provider and the providers price the model differently. Its cost is never shown as zero. | Add the row, with a valid-from date on or before the message, or have the collector send the provider, or put the message under a contract. See [Price tables](/reference/pricing-tables#unknown). |
| A message's model starts with a provider, such as `anthropic/claude-sonnet-4-5` | The provider is never read from the model's name. Without a provider, that name has no row. | Send the provider, or put the message under a contract; see [Model names](/reference/token-entry#model). |
| A message is labelled count error | It came without token counts by kind. | Have the collector send the counts; see [Choosing a collector](/collect#message). |
| A session seems to count twice, through telemetry and a session file | It does not: the two are one record by [session id and message id](/collect/telemetry#sees). | Nothing. |
| A session is on a feature it only mentioned | Rules cannot tell a mention from work. | Assign it by hand and tick remember. |
| A session is split equally between two features | It named both and nobody decided. | Split it by hand. |

## Before the cutoff {#cutoff}

| You see | Why | What to do |
| --- | --- | --- |
| A frozen figure did not follow a new price row, rule or default | Figures as of the [cutoff](/account/cutoff) are frozen. Rules run back only since the cutoff, and a contract rule that runs back further leaves frozen figures as they were. | Read the report as of today, or have an org manager [adjust](/account/cutoff#adjust) the event in before the cutoff. |
| A message arrived for a range before the cutoff | It waits, visible, dated when it arrived. | An org manager adjusts it in or reverses it. |
