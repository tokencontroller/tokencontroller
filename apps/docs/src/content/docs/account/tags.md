---
title: "Tags and the tag map"
description: "Tags on token entries, teams, the tag map for cost center and cost object, attribution and splits."
prev:
  link: "/account/organizations"
  label: "Organization settings"
next:
  link: "/account/projects"
  label: "Projects and tickets"
---

Every token entry carries tags, such as `team: web` or `client: acme`. Reports, token budgets and who sees what are all tag filters. The tag map says which tags fill the cost center and the cost object.

![Tags and the tag map: tag names with their values, and the ladders that fill cost center and cost object](/screens/13-tags-and-tag-map.png)

## Tags {#tags}

A tag is a name and a value. A token entry has at most one value per tag name, so cost grouped by one tag name always adds up to the total. An entry cannot be `team: web` and `team: data` at once. Work for two clients in one session is [split](#splits).

The controller keeps the list of tag names. Values are free: a new client or a new feature needs no setting. Three tag names are built in, because Token Controller keeps records behind them:

| Built-in tag | Value |
| --- | --- |
| `person` | The token entry's agent manager. Always set. |
| `ticket` | A [ticket](/account/projects#tickets), synced from your ticket system or created by hand, with its title, description and parent. |
| `project` | A [project](/account/projects), with its ticket key pattern and repositories. |

Every other tag name is yours: `team`, `department`, `client`, `feature`, `purpose`.

## Where tags come from {#sources}

| Source | Example |
| --- | --- |
| The person | A person tagged `team: web` passes it to their new token entries. |
| The project or ticket | A project tagged `client: acme` passes it to its token entries. |
| [Rules](/attribute/rules) | A branch named `KD-214-fix` sets `ticket: KD-214`. |
| A repository's config file | `.tokencontroller.toml` in a repository sets `project` or `purpose` for every session in it. See [Repository mapping](/attribute/repositories). |
| [Telemetry](/collect/telemetry) | Resource attributes in the agent's managed settings tag a whole team at once, with no install. |
| Mod, CLI, GitHub Action, endpoint | Tags sent with the token entry. |
| The person on their token sheet | Picks or corrects the ticket and any other tag before submitting. |

Inherited tags are copied when a token entry arrives. When someone changes team, their earlier token entries keep the old team, so last month's reports do not change.

## Teams and departments {#teams}

Teams, departments and disciplines are tags on people, such as `team: web` or `department: engineering`. Token Controller does not model an org chart. A person has one value per tag name: someone in two teams picks a main one, or the organization adds a second tag name such as `guild`. Teams are separate from projects on purpose, so one person can work on several projects whatever team they are in.

## The tag map {#map}

Inside, every share of cost has exactly one cost center and one cost object. The tag map says which tags fill them.

| Setting | Default | Example of a change |
| --- | --- | --- |
| Cost center | `person` | `team`, when teams answer for money. Each token entry still has one agent manager who submits it and is asked about it. |
| Ladder (cost object) | ticket, project, organization | ticket, epic, project, client, organization |

The **ladder** is read from the top. A token entry's cost object is the first rung with a value: no ticket, then its project; no project, then the organization. Rungs can be renamed, added and removed. A rung you add fills only if a project or ticket carries it, a rule sets it, or your ticket system sends it.

Each closed period keeps the tag map it was closed with. Changing the map changes the meaning of reports from then on, so compare periods across a change with care.

## How specific: the cost kind {#kinds}

Every token entry has a person, so all of it is attributed; only [residual](/account/bills#reconciliation), the part of a bill no token entry explains, is not. All other tags are optional and add detail. The cost kind says how much detail a split has, and follows from its tags:

| Cost kind | When |
| --- | --- |
| Direct | It has a ticket. |
| Project overhead | It has a project but no ticket. With extra rungs such as epic, everything between ticket and project counts here. |
| Organization overhead | It has no project. Its person, and any other tags, still apply. |

Reports, the Claude Code mod's band and the cost sheet show the share of each. How far an organization pushes towards tickets is its own choice: [rules](/attribute/rules) and people fixing exceptions move cost from organization overhead to projects and tickets.

## Splits {#splits}

Most token entries are one piece with one set of tags. A token entry with two sets of tags is split, and each split has its own tags:

- **By segment.** A session that changes branch or working directory is split where it changed, measured message by message. See [Sessions and subagents](/attribute/sessions).
- **By hand.** A person splits a token entry on their token sheet, for example 60 percent to ACME-50 and 40 percent to GLOBEX-7. These shares are a judgment, not a measurement.

## Filters {#filters}

A filter is a set of tags, such as `client: acme`, or `team: web` and `project: acme-shop`. A token entry matches when it has every tag in it. Filters are used for:

- [reports](/report/reports): a saved filter with a grouping, such as cost by team for `client: acme`
- [token budgets](/account/token-budgets): money planned for a filter and a period
- [people managers](/account/roles): each is assigned the filters they read and approve

Filters overlap. `team: web` and `client: acme` can count the same money, so two reports or two token budgets are never added together. Only grouping by one tag name, and the [cost sheet](/report/cost-sheet), add up to the total. Grouping by a tag that not every entry has shows a line for entries without it.

Tags do not touch the bill check. Residual, the part of a bill no token entry explains, has no tags, so a report filtered to one client never reaches the bill total.

## Keeping values clean {#values}

`acme`, `Acme` and `acme-corp` are three values. Ticket sync, rules and config files give clean values; hand-typed ones drift. A controller can merge values in an open period. After the close, only an [adjustment](/account/adjustments) changes anything.

## Where to next {#next}
