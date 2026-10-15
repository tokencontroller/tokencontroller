---
title: "Projects and tickets"
description: "Projects, ticket key patterns, repositories, tickets and syncing them from Jira Cloud, Azure DevOps or GitHub Issues."
---

Projects and tickets are the two built-in tags with records behind them, so they can carry a ticket key pattern, repositories, a title and a parent.

![A project with its ticket key pattern, repositories and tickets with their cost](/screens/12-project-and-tickets.png)

## What a project has {#project}

- a [ticket key pattern](#ticket-keys), such as `^KD-\d+`
- its [repositories](#repositories)
- tags, such as `client: acme`, which its token entries inherit
- optionally a token budget and a cadence

A client is a tag on projects, not a record of its own. Tag several projects `client: acme` and cost per client is a filter. To make client a level of the cost object, add it as a rung of the [ladder](/account/tags#map).

## Ticket key patterns {#ticket-keys}

Each project has one pattern, a regular expression that says how its tickets are named:

```text
^(KD|OPS)-\d+
```

For GitHub Issues, where tickets are numbers, use the repository name with the number, such as `storefront#\d+`. A project with no pattern gets one when you map a ticket system's project to it; see [Ticket systems](#ticket-systems).

Keys are found in the branch name, the pull request title, the session title and your prompts. Only the keys leave the machine, never the prompt. The Claude Code mod, the CLI and the GitHub Action need only the patterns, never your tickets; the service checks every key against the synced tickets.

If a key matches the pattern but the ticket has not synced yet, the token entry is placed on it anyway, and the ticket fills in at the next sync. If that sync still does not find it, the token entry shows in the review list as _unknown ticket_, and rules and the summary match run again without that key.

## Repositories {#repositories}

In the web app, open the project and add its repositories, such as `kestrel/storefront`. One repository belongs to one project.

Sessions in a mapped repository with no ticket key land on the project as project overhead. Sessions outside any mapped repository and without a ticket key stay organization overhead, with their person. This is the only placing that works with [telemetry](/collect/telemetry) alone, because telemetry sees the repository but not the branch.

A repository can also carry its own tags in `.tokencontroller.toml` at its root. Every session in it gets them, through the mod, the CLI or the GitHub Action:

```toml
[tags]
project = "storefront"
purpose = "product"
```

Only tag names on your organization's list are used. A ticket found by a rule still wins over the file.

## Tickets {#tickets}

A ticket is any unit of work: a feature, a story, an epic or a bug. It has a key and, where known, a title, a description, a parent ticket and an assignee. Only the key is required.

- **Synced** from Jira Cloud, Azure DevOps or GitHub Issues; see [Ticket systems](#ticket-systems).
- **By hand.** For work no ticket system knows, create a ticket in the web app. It works the same as a synced one.

A ticket can sit inside another, such as a story inside an epic. Cost rolls up: an epic's cost is the sum of its stories, and a project's cost is the sum of its tickets. A ticket's title and description are what [token estimates](/account/token-budgets#estimates) compare.

## Ticket systems {#ticket-systems}

An org manager connects a ticket system under _Settings_, then _Ticket systems_. All three work the same way:

| | Jira Cloud, Azure DevOps and GitHub Issues |
| --- | --- |
| Sign-in | An API token from the ticket system. No other sign-in is needed. |
| Mapping | Map each project of the ticket system to a Token Controller project. A project without a ticket key pattern gets the one its tickets use. |
| What syncs | Tickets with their titles, descriptions, parents and assignees. |
| When | Right after connecting or mapping, then every 15 minutes, or on demand. |
| Status | The list shows when each ticket system last synced, and why the last sync failed. |
| Disconnect | Its tickets stay and stop syncing. |
| Responsibility | Whoever connects a ticket system answers for that data flow. |

### Jira Cloud {#jira}

The address is your Jira Cloud site, such as `https://acme.atlassian.net`. Add the API token with the email of the Atlassian account it belongs to. Each Jira project maps by its key; project `KD` gets the pattern `^KD-\d+`.

### Azure DevOps {#azure-devops}

The address is your Azure DevOps organization, such as `https://dev.azure.com/acme` or `https://acme.visualstudio.com`. The token is a personal access token. Projects get the pattern `^AB#\d+`.

### GitHub Issues {#github-issues}

The address is `https://api.github.com`, or `https://api.<subdomain>.ghe.com` on GHE.com. The token is a personal access token or an app's token. Each repository is a project, written `owner/name`; `kestrel/storefront` gets the pattern `^storefront#\d+`. Pull requests are not synced as tickets.

## The assignee {#assignee}

The person a ticket is assigned to. Being assignee grants no access and costs nothing: the cost of a ticket sits with the cost center of each token entry on it.

## Cost that belongs to no ticket {#overhead}

Cost with a project but no ticket, such as research or setup, is project overhead. Cost with no project is organization overhead. Nobody sets this by hand; it follows from the tags. See [Cost kinds](/account/tags#kinds).
