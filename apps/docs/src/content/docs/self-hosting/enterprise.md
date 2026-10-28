---
title: "Enterprise plan instances"
description: "Isolated hosted instances, self-hosting with support, OIDC and SCIM, and what is not open source."
---

An Enterprise plan instance serves your organization alone, with its own service and its own database. The plan's features are listed under [Plans and limits](/plans#plans).

## Hosted for you {#hosted}

We run the instance in the region you choose. Nothing is shared with other organizations. Cloud bill connectors are set up on request.

## Self-hosted with support {#self}

A self-hosted instance with a support contract is an Enterprise plan instance. You keep your own Postgres and your own region. We send you the Enterprise image and a license key, with which the instance fetches the managed price table from us; the key and the other Enterprise settings are listed under [Configuration](/self-hosting/configuration#enterprise). [Talk to us](https://www.linkedin.com/in/jancbeck/).

## OIDC and SCIM {#oidc-scim}

Members can sign in with your identity provider, and SCIM keeps the member list in step with it.

### OIDC {#oidc}

Under _Settings_, _Sign-in_, add your provider's issuer URL, client ID and secret. Any OIDC provider works, such as Entra ID, Okta or Google Workspace. Register Token Controller with the provider as a web app with this redirect URI, where the address is your `TC_PUBLIC_URL`:

```text
https://tokencontroller.example.com/api/v1/sign-in/providers/oidc/callback
```

The provider must send the member's email address, as the `email` claim or, as Entra ID does, as `preferred_username`. Only members can sign in this way: invite people, or let SCIM add them.

### SCIM {#scim}

Create a SCIM token under _Settings_ and give it, with the SCIM URL, to your identity provider. The SCIM URL is your `TC_PUBLIC_URL` followed by `/scim/v2`. People you add or remove there are added or removed as members. Their role and attributes are set in Token Controller.

## What is not open source {#features}

Enterprise features and our own billing and support tools are not open source. They sit on top of the open-source service, and nothing in the open-source version depends on them. If an Enterprise plan ends, your data stays: people sign in by magic link, and every report and export keeps working. See [Open source and licenses](/reference/open-source).
