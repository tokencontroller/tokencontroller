---
title: "Tags and the tag map"
description: "Tags on token entries, teams, the tag map for cost center and cost object, cost kinds, splits and filters."
---

Every token entry carries tags, such as `team: web` or `client: acme`. Reports, token budgets and what people managers see are all [filters](#filters) on them.

![Tags and the tag map: tag names with their values, and the ladders that fill cost center and cost object](/screens/13-tags-and-tag-map.png)

## Tags {#tags}

A token entry has at most one value per tag name, so cost grouped by one tag name always adds up to the total. An entry cannot be `team: web` and `team: data` at once; work for two clients in one session is [split](#splits).

The organization manager keeps the list of tag names; values are free, so a new client needs no setting. Three tag names are built in and have records behind them:

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
| A repository's config file | `.tokencontroller.toml` sets `project` or `purpose` for every session in the repository. See [Repositories](/account/projects#repositories). |
| [Telemetry](/collect/telemetry) | Resource attributes in the agent's managed settings tag a whole team at once, with no install. |
| Mod, CLI, GitHub Action, endpoint | Tags sent with the token entry. |
| The person on their token sheet | Picks or corrects the ticket and any other tag before submitting. |

Inherited tags are copied when a token entry arrives. When someone changes team, their earlier token entries keep the old team, so last month's reports do not change.

## Teams and departments {#teams}

Teams and departments are tags on people, such as `team: web`; there is no org chart. With one value per tag name, someone in two teams picks a main one, or the organization adds a second tag name such as `guild`. Teams are separate from projects, so anyone can work on any project.

## The tag map {#map}

Every share of cost has exactly one cost center and one cost object.

| Setting | Default | Example of a change |
| --- | --- | --- |
| Cost center | `person` | `team`, when teams answer for money. Each token entry still has one agent manager who submits it and is asked about it. |
| Ladder (cost object) | ticket, project, organization | ticket, epic, project, client, organization |

The ladder is read from the top: the cost object is the first rung with a value. Rungs can be renamed, added and removed. A rung you add fills only if a project or ticket carries it, a rule sets it, or your ticket system sends it.

Each closed period keeps the tag map it was closed with. A change affects reports from then on, so compare periods across a change with care.

## Cost kinds {#kinds}

Every token entry has a person, so all of it is attributed; only [residual](/account/bills#reconciliation) is not. The cost kind says how specific a split is, and follows from its tags:

| Cost kind | When |
| --- | --- |
| Direct | It has a ticket. |
| Project overhead | It has a project but no ticket. With extra rungs such as epic, everything between ticket and project counts here. |
| Organization overhead | It has no project. Its person and other tags still apply. |

Reports, the Claude Code mod's band and the cost sheet show the share of each. [Rules](/attribute/rules) and people fixing exceptions move cost towards projects and tickets; how far is the organization's choice.

## Splits {#splits}

A token entry with two sets of tags is split, and each split has its own tags:

- **By segment.** A session that changes branch or working directory is split where it changed, measured message by message. See [Sessions and subagents](/attribute/rules#sessions).
- **By hand.** A person splits a token entry on their token sheet, for example 60 percent to ACME-50 and 40 percent to GLOBEX-7. These shares are a judgment, not a measurement.

## Filters {#filters}

A token entry matches a filter when it has every tag in it, such as `team: web` and `project: acme-shop`. Filters define:

- [reports](/report/reports): a saved filter with a grouping, such as cost by team for `client: acme`
- [token budgets](/account/token-budgets): money planned for a filter and a period
- what [people managers](/account/roles) read and approve

Filters overlap: `team: web` and `client: acme` can count the same money, so never add two reports or two token budgets together. Only grouping by one tag name, and the [cost sheet](/account/close#cost-sheet), add up to the total. Grouping by a tag that not every entry has shows a line for entries without it.

Residual has no tags, so a report filtered to one client never reaches the bill total.

## Keeping values clean {#values}

`acme`, `Acme` and `acme-corp` are three values. Ticket sync, rules and config files give clean values; hand-typed ones drift. An organization manager can merge values in an open period. After the close, only an [adjustment](/account/close#adjustments) changes anything.
