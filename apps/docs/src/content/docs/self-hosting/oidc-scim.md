---
title: "OIDC and SCIM"
description: "Sign in with your identity provider and keep members in step."
---

On the Enterprise plan, members can sign in with your identity provider, and SCIM keeps the member list in step.

## OIDC {#oidc}

Under _Settings_, _Sign-in_, add your provider's issuer URL, client ID and secret. Any OIDC provider works, such as Entra ID, Okta or Google Workspace.

Register Token Controller with your provider as a web app with this redirect URI, where the address is your `TC_PUBLIC_URL`:

```text
https://tokencontroller.example.com/api/v1/sign-in/providers/oidc/callback
```

The provider must send the member's email address, as the `email` claim or, as Entra ID does, as `preferred_username`. Only members can sign in this way: invite people, or let SCIM add them.

## SCIM {#scim}

Create a SCIM token under _Settings_ and give it with the SCIM URL to your identity provider. The SCIM URL is your `TC_PUBLIC_URL` followed by `/scim/v2`. People you add or remove there are added or removed as members. Their role and tags are set in Token Controller.
