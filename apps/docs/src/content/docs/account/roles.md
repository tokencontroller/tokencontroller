---
title: "Grants and roles"
description: "The seven capabilities, grants as (person, capability, filter), the three roles that bundle them, and what a new person gets."
---

A grant is (person, capability, filter): what a person may do, on which part of the organization. A role is a named bundle of grants. At sign-up there is one question: do you manage agents, or do you manage people?

## Capabilities {#capabilities}

| Capability | Allows | Filter |
| --- | --- | --- |
| View | Read cost and plan for the filter and every filter inside it. | Any |
| Record | Send messages and tag them with keys you were told. Autocomplete offers only values you have view on. | None: everyone, everything |
| Assign | Tag, split and correct other people's messages inside the filter. "Remember" a hand assignment as a rule. | Any |
| Approve | Approve or reject submissions inside the filter. | Any |
| Plan | Create estimates, accept plans and set shares inside the filter. | Any |
| Grant | Hand any of the above to someone else for a filter inside your own. | Any |
| Adjust | Add events dated before the cutoff. | Empty: org managers |

A filter is any set of tags and attributes; the empty filter is the whole organization. View on `team: web` reads `team: web` and `project: shop`, since that is inside it, but not `project: shop` alone.

## Roles {#roles}

| Role | Typically | Grants |
| --- | --- | --- |
| Agent manager | Anyone who uses an agent. | Record. View on `person: self`. |
| People manager | Tech leads and heads of department, for teams. Project managers and account managers, for projects and clients. | Agent manager, plus view, assign, approve, plan and grant on the filters they were granted, such as `team: web` or `client: acme`. |
| Org manager | Whoever answers for the numbers as a whole. | Every capability on the empty filter. Adjust. Moves the [cutoff](/account/cutoff). Sets up the organization. |

A new person gets record and view on `person: self`. A people manager hands out grants inside their own filter; an org manager anywhere. Granting is an assignment: a new grant for the same person and capability supersedes the old one, and a reversal takes it away.

<div class="note"><b>Examples.</b> A director of delivery is a people manager on the empty filter without adjust. A head of engineering is a people manager on <code>department: engineering</code>. An account manager is a people manager on <code>client: acme</code>. When two filters cover the same split, either may approve or assign it.</div>

## Who sees whom {#scope}

View reads cost per person inside the filter; no mode hides people behind team totals. See [Scope](/scope#out).

## The person of a message {#person}

Every message has exactly one person: you for your own sessions, and for a CI agent or other automation the person who created the service token it reports with. That person's attributes travel with the message. To hand an automation over, move its service token to another person; messages from then on carry the new person.

## Finance {#finance}

Finance needs no login. An org manager hands finance a [report](/report/reports) as a link or CSV, as of the cutoff.

## Roles and plans {#plans}

On the free Solo plan one person holds every grant. Grants for several people come with the Team plan; see [Plans and limits](/plans).
