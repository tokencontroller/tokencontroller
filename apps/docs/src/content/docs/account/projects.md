---
title: "Projects and tickets"
description: "The two built-in tags with records behind them: projects, clients, synced and manual tickets."
prev:
  link: "/account/tags"
  label: "Tags and the tag map"
next:
  link: "/account/roles"
  label: "Roles"
---

Projects and tickets are the two built-in tags with records behind them. Their values are real projects and tickets, so they can carry a ticket key pattern, repositories, a title and a parent.

![A project with its ticket key pattern, repositories and tickets with their cost](/screens/12-project-and-tickets.png)

## What a project has {#project}

- a [ticket key pattern](/attribute/ticket-keys), such as `^KD-\d+`
- its [repositories](/attribute/repositories)
- tags, such as `client: acme`, which its token entries inherit
- optionally a token budget and a cadence

A client is a tag on projects, not a record of its own. Tag several projects `client: acme` and cost per client is a filter. To make client a level of the cost object, add it as a rung of the [ladder](/account/tags#map).

## Tickets {#tickets}

A ticket is any unit of work: a feature, a story, an epic or a bug. It has a key and, where known, a title, a description, a parent ticket and an assignee. Only the key is required.

- **Synced.** Connect [Jira Cloud](/integrations/jira), [Azure DevOps](/integrations/azure-devops) or [GitHub Issues](/integrations/github-issues). Tickets sync every 15 minutes, or when you ask.
- **By hand.** For work no ticket system knows, create a ticket in the web app. It works the same as a synced one.

A ticket can sit inside another, such as a story inside an epic. Cost rolls up: an epic's cost is the sum of its stories, and a project's cost is the sum of its tickets. A ticket's title and description are what [token forecasts](/account/token-budgets#forecasts) compare.

## The assignee {#assignee}

The person a ticket is assigned to. Being assignee costs nothing: the cost of a ticket sits with the cost center of each token entry on it.

## Cost that belongs to no ticket {#overhead}

Cost with a project but no ticket, such as research or setup, is project overhead. Cost with no project at all is organization overhead. Nobody sets this by hand; it follows from the tags.

## Where to next {#next}
