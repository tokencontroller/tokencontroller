---
title: "Organization settings"
description: "What an org manager sets up once, inviting people, and how members sign in."
---

When you start with Token Controller, you set up one organization: one event log of its people, tag values, messages, bills and plans, with one cutoff and one reporting currency. An org manager sets it up once.

## Organization settings {#settings}

| Setting | What it does |
| --- | --- |
| Reporting currency | The one currency figures are shown in. The price table is in US dollars; a bill keeps its own currency and the exchange rate of its last day. |
| Price table | The managed table, with your own rows, for every contract or for one. See [Price table](/account/price-table). |
| Tag names and tag values | Projects, features, clients and contracts, your own tag names, and the Jira connection features may sync from. See [Tags](/account/tags). |
| Attribute names | Team, department, location and their defaults. See [Attributes](/account/attributes). |
| Organization rules | Rules that run before everyone's personal rules. See [Rules](/attribute/rules). |
| Approval | Whether submissions need approval. See [Submit and approval](/account/submit). |
| Cutoff | The date up to which figures are frozen. See [Cutoff](/account/cutoff). |
| Sign-in | Which [sign-in](#sign-in) methods members may use, and which email domains. |
| Content text | Whether the usage endpoints keep prompts, responses, tool output and file contents in what they received. Off by default; set through the [API](/reference/api/organization). See [Security and privacy](/security#never). |

The name, the reporting currency, the sign-in domains and approval are under _Settings_ in the web app. The other settings have their own screens.

## People {#people}

Invite people by email. Each gets a [role](/account/roles) and their [attributes](/account/attributes), such as `team: web`. An attribute value has a from date, so when a person changes team, messages before that date keep the old team and figures as of an earlier date do not change.

## Sign-in {#sign-in}

| Method | Plans |
| --- | --- |
| Magic link by email | All. Can be limited to your email domains. |
| OIDC and SCIM | Enterprise plan. See [OIDC and SCIM](/self-hosting/enterprise#oidc-scim). |

A magic link works once and for 15 minutes. Opening it shows a page with one button, so a mail scanner that opens the link does not use it up.

Creating an organization takes one email. Its link creates the organization and signs you in, in whichever browser opens it, with the answers you gave at sign-up. If you ask to sign in with an address that has no account, the email says so, and its link opens the sign-up form with your address already confirmed.
