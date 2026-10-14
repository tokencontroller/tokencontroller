---
title: "Data model"
description: "The records Token Controller keeps and the rules that hold between them."
---

The records Token Controller keeps, and the rules that hold between them. The terms are defined in the [glossary](/glossary); [How it works](/concepts) shows them together.

## Records {#records}

| Record | What it holds |
| --- | --- |
| Organization | Cost source, approval on or off, members and their roles, reporting currency, tag names, tag map. |
| Person | A member. The default cost center. Carries tags, such as `team`, that its token entries inherit. |
| Project | Tags such as `client`, mapped repositories, ticket key pattern. |
| Ticket | Synced from a ticket system or created by hand. A key; optionally a title, a description, a parent ticket and an assignee. |
| Token entry | Agent harness, optional agent name, model, tokens by kind, optional reported cost, list value, billed value, start and end time, summary, agent manager, splits, whether it arrived automatically or on a token sheet, status, evidence. See [Token entry schema](/reference/token-entry). |
| Split | One share of a token entry with its own tags. Most token entries are one split. |
| Tag | A tag name and a value on a split, a person, a project or a ticket. One value per tag name. Built in: `person`, `ticket`, `project`. All optional except `person`. |
| Tag map | Which tag fills the cost center, and the ladder of tags that fills the cost object. |
| Filter | A set of tags. Reports, token budgets and people managers' access are filters. |
| Token sheet | One agent manager, one cadence period, their reviewed token entries. Submitted or late. |
| Cadence | The rhythm in which token sheets are due, set per project or organization by a people manager. |
| Token budget | Money a human has planned for a filter and a period. |
| Token forecast | Money suggested for a ticket from similar past work, with the matched tickets and a confidence. Becomes a token budget when a person accepts it. |
| Telemetry event | One model call. Joins to token entries by session and message ID. |
| Cost source | One way a provider charges: subscription, seat, usage at API rates, API key, cloud marketplace or reseller. |
| Bill | A provider's figure for one period: gross, discount, credit, net, currency and exchange rate, cost source, revision. See [Bill schema](/reference/bill). |
| Period | One calendar month, open or closed. |
| Cost sheet | The locked account of one closed period, adding up to the bill. |
| Rule | Ordered. Organization rules run before personal ones. |

There is no agent record. An agent is whatever caused a token entry, described on the entry by agent harness, agent name and model.

## Token entry status {#status}

| Status | Meaning |
| --- | --- |
| Draft | Still on the machine, waiting for its token sheet. |
| Submitted | Counts in reports. Automatic token entries start here. |
| Approved | A people manager accepted it. Only used when approval is on. |
| Rejected | Sent back to its agent manager with a reason. Does not count until fixed. |
| Locked | Its period is closed. |

## Rules that always hold {#rules}

<ol class="steps">
  <li><b>Every cost has a cost center and a cost object.</b><p>Both come from tags through the tag map. The cost center is by default the token entry's agent manager. The cost object is the first rung of the ladder with a value, by default ticket, then project, then organization.</p></li>
  <li><b>No token entry without an agent manager.</b><p>One that has none shows as an error to people managers until someone takes it.</p></li>
  <li><b>One value per tag name.</b><p>A split never carries two values of the same tag name, so cost grouped by one tag name adds up to the total.</p></li>
  <li><b>History stays as it was.</b><p>When an automation gets a new agent manager, or a person, project or ticket changes its tags, only future token entries follow the change. Past splits keep the agent manager and tags they had. A closed period keeps the tag map it closed with.</p></li>
</ol>
