---
title: "Sign-in and status"
description: "Magic links for the web app and local tools, sign-up, who am I, invite links, and the health and status endpoints that need no sign-in."
---

The web app and `tc connect` sign in with a magic link sent by mail. There is no password. The Enterprise plan adds OIDC; see [OIDC and SCIM](/self-hosting/enterprise#oidc-scim).

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /sign-in` | Send a sign-in link to a member. | Nobody signed in |
| `GET /sign-in/link`, `POST /sign-in/link` | The page a link opens, and the form on it that uses the link. | The browser |
| `POST /sign-in/poll` | Collect a local tool's session after the person used the link. | A local tool |
| `GET /sign-in/providers` | The ways to sign in besides the magic link, for the buttons on the sign-in page. | Nobody signed in |
| `POST /sign-out` | End the session in the cookie or the bearer token. | Signed in |
| `POST /sign-up` | Create an organization and mail its first org manager a link. | Nobody signed in |
| `GET /sign-up/link`, `POST /sign-up/link` | The address behind a "no account" link; create the organization with it. | The browser |
| `GET /me` | The signed-in person and their organization. | Signed in, or a read token |
| `GET /invites/{code}` | What an invite link is for, before sign-in. | Nobody signed in |
| `GET /health` | Is the service up and can it reach its database? | Nobody signed in |
| `GET /status` | The state of every part status.tokencontroller.com shows. | Nobody signed in |

## Magic links {#links}

```http
POST /api/v1/sign-in

{ "email": "mara@kestrel.example" }
```

Always `202 Accepted`, so nobody learns whether an address is a member. A member gets a link that signs them in; an address that belongs to no organization gets a mail that says so, with a link to create one. `400` for text that is not an email address, `429` after too many links for one address.

The link opens `GET /sign-in/link?token=…`, a page with one _Sign in_ button, so a mail scanner that opens the link does not use it up. The button posts the token to `POST /sign-in/link`; the browser gets its `tc_session` cookie and goes to the web app (`303`). `410` when the link was used, has expired or never existed.

A local tool such as `tc connect` sends `{"email": "…", "local_tool": true}` and gets a `poll_token` back. It then asks `POST /sign-in/poll` with `{"poll_token": "…"}`: `202` with `status: "waiting"` until the person used the link, then `200` once with `status: "signed_in"` and `session_token`, the `tc_cli_…` token to send as `Authorization: Bearer`. `410` once the session was collected or the link expired.

## Sign-up {#sign-up}

```http
POST /api/v1/sign-up

{ "email": "nadia@kestrel.example", "organization_name": "Kestrel Digital", "name": "Nadia Okafor", "reporting_currency": "EUR", "manages": "people" }
```

`202`: a link is on its way, and the organization exists once it is used, in any browser. `manages` is `agents` or `people` and picks the first steps the web app shows. `reporting_currency` defaults to `USD`. An address that is already a member gets an ordinary sign-in link instead: one account belongs to one organization. `POST /sign-up/link` with `{"token": "…", "organization_name": "…"}` creates the organization from the link in the "no account" mail and signs the founder in at once (`204`, cookie set).

## Who am I {#me}

```json
{
  "credential": "session",
  "person": { "id": "7f1c4e2a-…", "name": "Mara Lind", "email": "mara@kestrel.example", "role": "agent_manager", "filters": [], "attributes": { "team": "web" }, "status": "joined", "invited_at": "2026-01-08T09:20:00Z", "joined_at": "2026-01-08T09:31:00Z" },
  "organization": { "id": "…", "name": "Kestrel Digital", "slug": "kestrel" },
  "org_manager_notice": null
}
```

`credential` is `session` for a person and `read` for a read token. `org_manager_notice` is a sentence for org managers to show on every page, such as an Enterprise plan renewal, and `null` otherwise.

`GET /invites/{code}` takes the part after `/i/` in an invite link and answers `{"email": "mara@kestrel.example", "organization_name": "Kestrel Digital"}`, so the sign-in page can fill the address in.

## Health and status {#status}

`GET /health` answers `{"status": "ok", "database": "ok", "version": "1.0.0", "edition": "open"}`, or `503` with a check `failing`. `GET /status` is what [status.tokencontroller.com](https://status.tokencontroller.com) shows:

```json
{
  "status": "operational",
  "version": "1.0.0",
  "components": [
    { "id": "service", "state": "operational", "last_success_at": null },
    { "id": "bill-pulls", "state": "operational", "last_success_at": "2026-04-05T06:10:00Z" },
    { "id": "ticket-sync", "state": "operational", "last_success_at": "2026-04-05T07:15:00Z" },
    { "id": "submit-reminders", "state": "operational", "last_success_at": "2026-04-05T07:00:00Z" },
    { "id": "price-table", "state": "operational", "last_success_at": "2026-04-05T07:10:00Z" },
    { "id": "exchange-rates", "state": "operational", "last_success_at": "2026-04-05T02:00:00Z" },
    { "id": "sign-in-email", "state": "operational", "last_success_at": null }
  ]
}
```

A state is `operational`, `degraded`, `down` or `not_running`; `status` is the worst state of any running part.
