---
title: "Roles"
description: "Agent manager, people manager and controller: who sees what and who can do what."
prev:
  link: "/concepts"
  label: "Concepts"
next:
  link: "/account/approval"
  label: "Approval"
---

Roles decide what you see. The two sides decide where cost sits. At sign-up there is one question: do you manage agents, or do you manage people?

![People: members with their role and tags](/screens/15-people.png)

## The three roles {#roles}

Roles stack: each one includes everything the one before it can do. A member is anyone in the organization, whatever their role.

| Role | Who has it | What it can do |
| --- | --- | --- |
| Agent manager | Anyone who uses an agent: developers, designers, QA, architects, managers. | Reviews and submits their own token sheets. Sees their own cost. |
| People manager | Tech leads and heads of department for people and teams. Project managers, directors of delivery and account managers for projects and clients. | Reads reports for the filters they are assigned, such as `team: web`, `project: acme-shop` or `client: acme`. Sets token budgets and cadence there. Approves token entries, or rejects them back with a reason. |
| Controller | Whoever answers for the numbers as a whole. | Sees everything. Sets up the organization: cost source, reporting currency, bills, ticket sync, organization rules, approval on or off. Runs the monthly close. Posts adjustments to closed periods. |

<div class="note"><b>Examples.</b> A director of delivery is a people manager over all projects. A head of engineering is a people manager over <code>department: engineering</code>. An account manager is a people manager over <code>client: acme</code>. When two filters cover the same token entry, either people manager can approve it.</div>

## What a people manager sees {#scope}

A people manager sees cost per person within what they are assigned. A controller sees cost per person for the whole organization. There is no mode that hides people behind team totals; see [Scope](/scope).

## The agent manager of a token entry {#agent-manager}

Every token entry has exactly one agent manager, who answers for it: submits it, gets asked about it, and sees it. For your own sessions that is you. For a CI agent or another automation it is the person who created the service token it reports with. The agent manager is the default [cost center](/account/tags#map); an organization that maps the cost center to `team` rolls the money up to the team, and the agent manager still answers for the entry.

If an automation's service token belongs to nobody, for example because its creator left, its token entries show as an error to people managers until someone takes it over. To hand an automation over, move its service token to another person in the web app. Only future token entries follow.

## Relationships that are not roles {#not-roles}

A ticket has an **assignee**, the person it is assigned to. Being an assignee grants nothing and costs nothing: the cost of a ticket sits with the cost center of each token entry on it.

## Finance {#finance}

Finance needs no login and sets nothing up. A controller hands finance the [cost sheet](/report/cost-sheet) or a CSV export.

## Roles and plans {#plans}

On the free Solo plan one person holds every role. Roles for several people come with the Team plan; see [Plans and limits](/plans).

## Where to next {#next}
