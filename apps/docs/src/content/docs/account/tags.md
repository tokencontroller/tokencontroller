---
title: "Tags"
description: "Tag values as records with defaults, the built-in tag names project, feature, client and contract, your own tag names, and syncing features from Jira."
---

You may want to know what your agents' cost was for: which project, which feature, which client, and which contract paid for it. A tag says that. It is a tag name with a tag value, such as `project: shop` or `feature: KD-214`. Tags go on messages through splits, and on bills through their contract. Reports, plans and grants are all [filters](#filters) on tags and [attributes](/account/attributes).

## Tag values are records {#values}

Every tag value is a record, never a bare string: `shop` under `project` exists once, with its key, an optional title and description, a source id when a tag source such as Jira owns it, and its default tags. `acme`, `Acme` and `acme-corp` are three values until someone reverses two of them. Anyone with [record](/account/roles#capabilities) can tag a message with a key they were told; autocomplete offers only values they have view on.

| Field | Meaning |
| --- | --- |
| Tag name | `project`, `feature`, `client`, `contract` or one of your own. |
| Key | The short name used in tags and on the command line, such as `KD-214` or `shop`. |
| Title, description | Optional. A feature's description is what the [estimator](/account/estimates) reads. |
| Source id | The id in the tag source that owns it, such as a Jira issue id. Set by sync. |
| Default tags | The tags this value brings with it; see [Defaults](#defaults). |

## The built-in tag names {#built-in}

| Tag name | Value | Default tags |
| --- | --- | --- |
| `project` | A body of work. | Usually a `client`, and may name a `contract`. |
| `feature` | Any unit of work: a ticket, a story, an epic, a task, a bug are all features. Synced from Jira or created by hand. | Its `project`. |
| `client` | Who the work is for. | None. |
| `contract` | One thing a provider charges you under, such as an API account or a seat plan: its provider, metered or flat, and the fee when flat. A message has at most one; a bill has exactly one. | None. |

You rarely create a contract by hand: connecting or uploading a bill creates its contract, and a shipped sign-in rule creates one the first time it matches. How a message gets its contract is on [Bills](/account/bills#reconciliation) and the [data model](/reference/data-model#contract) page.

Every other tag name is yours: `purpose`, `environment`, anything a rule or a person can set. An org manager keeps the list of tag names; anyone with assign on a filter adds values inside it.

## Defaults {#defaults}

A tag value brings its default tags when a figure is computed: a feature brings its project, a project brings its client. There is no setting that says which tag is above which; the defaults do it. A filter on `client: acme` finds every split on a feature of one of Acme's projects, and cost per project is the sum of its features plus the project's own overhead.

Defaults are applied as of the report's date, so changing a project's client changes figures as of a later date and never figures as of the cutoff.

## Projects {#projects}

Create a project under _Tags_, give it a client as its default tag, and add a [rule](/attribute/rules) for its repositories, such as "repository `kestrel/shop` is project Shop". Sessions in that repository without a feature are project overhead on Shop. With [telemetry](/collect/telemetry) alone, this is the placing that works, because telemetry sees the repository but not the branch.

## Features {#features}

A feature has a key and, where known, a title, a description and its project as default. Only the key is required.

- **Synced** from Jira; see below.
- **By hand.** For work Jira does not know, such as an issue in Azure DevOps or GitHub Issues, create a feature under its project, here or through the [API](/reference/api/tags). It works the same as a synced one.

An epic, a story and its tasks are all features. A story's default tags name its project, not the epic: cost per epic is a filter on the epic's own feature value, and a session on the story counts on the story. Tag the session with both to count it on both.

[Rules](/attribute/rules) find keys in the branch name, the pull request title and the prompts. Of a prompt, only the keys leave the machine. A key that matches a rule but names no feature yet leaves the message with no project, visible to whoever holds view there; assigning it once can become a rule.

## Jira {#jira}

Connecting Jira is optional. With it, every feature is a copy of its Jira issue, kept in step every 15 minutes, or when someone chooses _Sync now_. An org manager connects it under _Tags_, _Connect Jira_.

| | Jira Cloud |
| --- | --- |
| Address | Your site, such as `https://acme.atlassian.net`, with an API token and the email of its Atlassian account. |
| Sync map | Map each Jira project to a Token Controller project. Its issues become features with that project as default. |
| Fields | Key, title, description, type, status, parent and assignee are copied. Through the [API](/reference/api/tags#ticket-systems), a sync map can list fewer, such as leaving out the description. |
| Status | The connection shows when it last synced and why the last sync failed. |
| Disconnect | Its features stay and stop syncing. |
| Responsibility | Whoever connects Jira answers for that data flow. |

A rule such as "key `^KD-\d+` in the branch is that feature" finds the feature by key; the synced copy fills in its title and project.

## Labels {#labels}

Nobody sets a label; it follows from the tags of a split:

| Label | When |
| --- | --- |
| Direct | It has a feature. |
| Project overhead | It has a project but no feature. |
| Organization overhead | It has no project. Its person and attributes still apply. |

Reports and the Claude Code mod's band show the share of each. [Rules](/attribute/rules) and people fixing exceptions move cost towards features; how far is your choice.

## Filters {#filters}

A filter is any set of tags and attributes, such as `client: acme`, or `team: web` and `project: shop`. A split matches when its full tag set, with defaults, the message's contract and the person's attributes, contains every tag in the filter. Filters define:

- [reports](/report/reports): cost of a filter, grouped by any tag name or attribute name
- [plans](/account/estimates): the accepted estimate for a filter
- [grants](/account/roles): what a person may view, assign, approve or plan

For one tag name, the values partition the total. Filters on different names overlap: `team: web` and `client: acme` can count the same money, so never add two filters together. See [Checking a number](/report/checking-a-number).
