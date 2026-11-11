---
title: "Cutoff"
description: "The one date per organization up to which figures are frozen, how an org manager moves it, and how corrections before it work."
---

Once you have given figures to finance, you may want them to stay as they were, while new messages and bills keep arriving. The cutoff is one date per organization. Every figure as of the cutoff is frozen; everything after it is live. There are no months to finish one by one.

## What the cutoff does {#what}

| | Before the cutoff | After the cutoff |
| --- | --- | --- |
| New messages, splits, accepts, bills | Only an org manager with [adjust](/account/roles#capabilities) may add events dated there. | Anyone with the capability. |
| Rules | Run back over unassigned messages only since the cutoff. Rules on `contract` may run back before it; figures as of the cutoff stay as they were. | Run on arrival and back over unassigned messages. |
| Reports as of the cutoff | Frozen: the same answer every time. | Change as events arrive. |

Every report shows its as-of date. A report as of today includes everything; a report as of the cutoff is the one finance gets.

## Moving it {#move}

An org manager moves the cutoff under _Cutoff_ in the web app. Before it moves, the screen lists what may need a look first:

- the [check](/account/bills#reconciliation) of every bill that ends in the range: what no message explains must be zero or positive, and for a flat contract, covered by the plan
- organization overhead since the cutoff, which may belong on a project
- [submissions](/account/submit) not approved yet, when your organization approves

None of these blocks the move. Moving the cutoff back is allowed and is an event like any other.

## Corrections before the cutoff {#adjust}

Nothing is edited. A wrong split before the cutoff gets a reversal and a new split, both dated when they happen; an org manager with adjust can date them before the cutoff instead, so the frozen figures change on purpose and the log says who did it and when. A message that arrives late for a range before the cutoff waits, visible, until an org manager adjusts it in or reverses it.
