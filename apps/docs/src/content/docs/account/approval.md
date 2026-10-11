---
title: "Approval"
description: "Approval on or off, and rejecting token entries with a reason."
---

Approval lets people managers accept submitted token entries before the period closes. Each organization switches it on or off.

## With approval off {#off}

Submitted token entries count in reports at once. People managers can still reject one.

## With approval on {#on}

Submitted token entries still count in reports at once, marked as not yet approved, and reports can filter on that. A people manager approves token entries for the filters they are assigned, such as `team: web` or `client: acme`, one by one or in bulk after filtering. Automatic token entries, such as CI, are approved the same way. A period cannot be closed while a token entry in it is unapproved, so the cost sheet holds approved cost only.

## Reject {#reject}

A people manager sends a token entry back to its agent manager with a reason. A rejected token entry does not count until it is fixed and submitted again. It shows in the agent manager's [review list](/attribute/review).
