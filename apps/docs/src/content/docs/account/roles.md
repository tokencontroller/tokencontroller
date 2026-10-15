---
title: "Roles"
description: "Agent manager, people manager and org manager: who sees what and who can do what."
---

Roles decide what you see and what you can change. At sign-up there is one question: do you manage agents, or do you manage people?

![People: members with their role and tags](/screens/15-people.png)

## The three roles {#roles}

Each role includes everything the one above it can do.

| Role | Typically | What it can do |
| --- | --- | --- |
| Agent manager | Anyone who uses an agent: developers, designers, QA, architects, managers. | Reviews and submits their own token sheets. Sees their own cost. |
| People manager | Tech leads and heads of department, for people and teams. Project managers, directors of delivery and account managers, for projects and clients. | For the filters they are assigned, such as `team: web` or `client: acme`: reads reports, sets token budgets and cadence, and approves token entries or rejects them with a reason. |
| Org manager | Whoever answers for the numbers as a whole. | Sees everything. Sets up the organization: cost source, reporting currency, bills, ticket sync, organization rules, approval on or off. Runs the monthly close. Posts adjustments to closed periods. |

<div class="note"><b>Examples.</b> A director of delivery is a people manager over all projects. A head of engineering is a people manager over <code>department: engineering</code>. An account manager is a people manager over <code>client: acme</code>. When two filters cover the same token entry, either people manager can approve it.</div>

## What a people manager sees {#scope}

Cost per person within the filters they are assigned; an org manager sees it for the whole organization. No mode hides people behind team totals; see [Scope](/scope#out).

## The agent manager of a token entry {#agent-manager}

Every token entry has exactly one agent manager, who submits it, is asked about it and sees it. For your own sessions that is you. For a CI agent or other automation it is the person who created the service token it reports with. The agent manager is the default [cost center](/account/tags#map); when the cost center is `team`, the money rolls up to the team and the agent manager still answers for the entry.

If an automation's service token belongs to nobody, for example because its creator left, its token entries show as an error to people managers until someone takes it over. To hand an automation over, move its service token to another person in the web app. Only future token entries follow.

## Relationships that are not roles {#not-roles}

A ticket's [assignee](/account/projects#assignee) gets no access and no cost from being assignee.

## Finance {#finance}

Finance needs no login. An org manager hands finance the [cost sheet](/account/close#cost-sheet) or a CSV export.

## Roles and plans {#plans}

On the free Solo plan one person holds every role. Roles for several people come with the Team plan; see [Plans](/plans).
