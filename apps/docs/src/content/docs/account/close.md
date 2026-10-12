---
title: "Monthly close"
description: "Closing a period and what happens after."
---

The close locks a period, a calendar month, so its token entries and their tags can no longer change. The period keeps the tag map it was closed with. It produces the cost sheet.

## Closing a period {#steps}

<ol class="steps">
  <li><b>Check the bill.</b><p>The period's bill is in, from the API, as CSV or pulled, and final. Some pulled bills take weeks to be final, such as <a href="/integrations/anthropic-enterprise">Anthropic Enterprise</a>, and the close waits for them.</p></li>
  <li><b>Look at what is open.</b><p>Organization overhead that should be on a project, late token sheets, and with approval on, unapproved token entries.</p></li>
  <li><b>Send back what is wrong.</b><p>Reject a token entry to its agent manager with a reason.</p></li>
  <li><b>Close.</b><p>A controller closes the period. Token entries and residual add up to the bill.</p></li>
</ol>

With approval on, a period cannot be closed while a token entry in it is unapproved.

## After the close {#after}

Token entries in the period are locked. The period keeps the price table it was closed with. Corrections are [adjustments](/account/adjustments). A token sheet submitted after the close becomes an adjustment to that month, marked late. Hand finance the [cost sheet](/report/cost-sheet).
