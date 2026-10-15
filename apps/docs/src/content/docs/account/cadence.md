---
title: "Token sheets and approval"
description: "Token sheets, their cadence and what late means, and approving or rejecting token entries."
---

A token sheet is how a person confirms which ticket each of their sessions was for and what it did. A cadence says how often token sheets are due, and approval lets people managers accept what was submitted.

## Token sheets {#token-sheet}

A token sheet holds one agent manager's reviewed token entries for one cadence period. The agent manager [reviews](/attribute/review) the drafts, fixes the ticket where a rule got it wrong, reads the summaries, and submits whenever they like. Cloud and CI token entries never go on a token sheet. Telemetry may already have reported the money of the same sessions; the sheet adds the ticket and the summary, and the two are joined by session ID and counted once.

## Cadence {#cadence}

A people manager can set a cadence for a project or the whole organization: weekly, every two weeks, monthly, or custom. The point is that people report while they still remember what the work was for, not three weeks later.

## Late {#late}

A token sheet whose cadence period is over and that has not been submitted is late. The only consequence is a reminder to its agent manager: one email when it becomes late, and a note in the Claude Code mod and in the web app. Nobody is blocked and nothing is capped. Before the monthly close, the org manager can ask everyone to submit, the way finance asks for timesheets before month end. A sheet that comes in after the close is posted to that month as an [adjustment](/account/close#adjustments), marked late.

## Approval {#approval}

Each organization switches approval on or off.

![Approval: submitted token entries of the people a people manager answers for](/screens/04-approval.png)

| | Approval off | Approval on |
| --- | --- | --- |
| Submitted token entries in reports | Count at once | Count at once, marked as not yet approved; reports can filter on that |
| On the [cost sheet](/account/close#cost-sheet) | Submitted cost | Approved cost only |
| Closing a period | Approval plays no part | Blocked while a token entry in the period is unapproved |

With approval on, a people manager approves token entries for the filters they are assigned, such as `team: web` or `client: acme`, one by one or in bulk after filtering. Automatic token entries, such as CI, are approved the same way.

### Reject {#reject}

With approval on or off, a people manager can send a token entry back to its agent manager with a reason. A rejected token entry does not count until it is fixed and submitted again. It shows in the agent manager's [review list](/attribute/review).
