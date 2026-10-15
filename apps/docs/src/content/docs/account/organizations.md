---
title: "Organization settings"
description: "What an organization manager sets up once, inviting people, and how members sign in."
---

An organization is one customer of Token Controller: its people, projects, tags and periods. An organization manager sets it up once.

![Settings: name, cost source, reporting currency, approval, cadence and sign-in domains](/screens/16-settings.png)

## Organization settings {#settings}

| Setting | What it does |
| --- | --- |
| Cost source | How your provider charges you. It decides what the close reconciles against; see [Bills](/account/bills). |
| Reporting currency | The one currency the organization computes in. Bills in other currencies are booked at the exchange rate of their date. |
| Tag names and tag map | The tag names your organization uses, which tag is the cost center and the ladder for the cost object. See [Tags and the tag map](/account/tags). |
| Ticket systems | Where tickets sync from. See [Ticket systems](/account/projects#ticket-systems). |
| Organization rules | Rules that run before everyone's personal rules. See [Rules](/attribute/rules). |
| Approval | On or off. See [Approval](/account/cadence#approval). |
| Sign-in | Which [sign-in](#sign-in) methods members may use, and which email domains. |

## People {#people}

Invite people by email. Each gets a [role](/account/roles) and their tags, such as `team: web`. When a person changes team or leaves, only their future token entries follow; last month's reports do not change.

## Sign-in {#sign-in}

| Method | Plans |
| --- | --- |
| Magic link by email | All. Can be limited to your email domains. |
| OIDC and SCIM | Enterprise plan. See [OIDC and SCIM](/self-hosting/enterprise#oidc-scim). |

A magic link works once and for 15 minutes. Opening it shows a page with one button, so a mail scanner that opens the link does not use it up.

Creating an organization takes one email. Its link creates the organization and signs you in, in whichever browser opens it, with the answers you gave at sign-up. If you ask to sign in with an address that has no account, the email says so, and its link opens the sign-up form with your address already confirmed.
