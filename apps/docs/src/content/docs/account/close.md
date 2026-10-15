---
title: "Closing a period and the cost sheet"
description: "Closing a period, the cost sheet it produces, and adjustments after the close."
---

The close locks a period, a calendar month, so its token entries and their tags can no longer change, and produces the cost sheet.

![Monthly close: the checks before the close and the periods below](/screens/07-monthly-close.png)

## Closing a period {#steps}

<ol class="steps">
  <li><b>Check the bill.</b><p>The period's bill is in, from the API, as CSV or pulled, and final. Some pulled bills take weeks to be final, such as <a href="/account/bills#anthropic-enterprise">Anthropic Enterprise</a>, and the close waits for them.</p></li>
  <li><b>Look at what is open.</b><p>Organization overhead that should be on a project, late token sheets, and with approval on, unapproved token entries.</p></li>
  <li><b>Send back what is wrong.</b><p>Reject a token entry to its agent manager with a reason.</p></li>
  <li><b>Close.</b><p>An organization manager closes the period. Token entries and residual add up to the bill.</p></li>
</ol>

With [approval](/account/cadence#approval) on, a period cannot be closed while a token entry in it is unapproved.

## After the close {#after}

Token entries in the period are locked. The period keeps the tag map and the price table it was closed with. Corrections are [adjustments](#adjustments). A token sheet submitted after the close becomes an adjustment to that month, marked late.

## The cost sheet {#cost-sheet}

The cost sheet is the locked account of one closed period for the whole organization: cost per project and ticket, adding up to the bill.

![The cost sheet of a closed month: cost per project and ticket, overhead and the bill](/screens/08-cost-sheet.png)

It shows:

- cost per rung of your [ladder](/account/tags#map), by default per project and ticket, by cost kind
- project overhead and organization overhead
- residual, as its own line
- adjustments posted after the close
- the reconciliation against the [bill](/account/bills#reconciliation)

With approval on, it holds approved cost only.

### For finance {#finance}

An organization manager hands finance the cost sheet as a link or as CSV. The CSV can add any tag name as a column, such as `team` or `purpose`. Finance needs no login.

### For your clients {#clients}

Token Controller ends at the cost sheet. Charging a client for what is on it happens in your own invoicing tool.

## Adjustments {#adjustments}

A closed period does not change. A mistake found later is corrected with an adjustment: a signed correction an organization manager posts to the closed period. The organization manager chooses the period, the cost object, the amount (plus or minus) and a reason, and the adjustment carries their name and the time.

Adjustments are their own lines on the cost sheet and in reports. The original token entries and their tags stay as they were.
