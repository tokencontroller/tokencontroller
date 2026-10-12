# The public repo holds the product only

Supersedes ADR 0014.

This repo holds the product: the service, the web app, the CLI, the mods, the GitHub Action, the SDK and the docs. Our own business code does not live here. Team plan billing and customer relations (the support inbox, the contact form, support inside the app, the site assistant, the newsletter, feedback reports, the admin area, the customer record and onboarding emails) live in the private repo, next to the Enterprise plan features (ADR 0001).

## Rules that follow

- Billing is a private Rust crate. The hosted image links it with this service through the plug-in points and is built from the open image of the same commit.
- Customer relations is its own app on its own infrastructure. It never connects to the service's database; it reads what it needs from the hosted service.
- The plug-in points stay generic and name no private code: `Extension` in the core crate for hooks that need no database, `ServiceHooks` in `apps/server/src/hooks.rs` for hooks that do (the member limit), `serve_with_additions`, `router_with` and `openapi_json_with` for a build's own routes, and the web app's `src/additions/` slot for its own pages, menu entries and page parts.
- The migrations of the removed code left with it. Databases that ran them still start: the service's migrator passes over applied migrations it no longer has. Nothing is dropped here; the old tables stay until the hosted service drops them.
- A self-hosted instance has no feedback button, no support dialog and no billing page. The docs say which parts only the hosted service has.

## Why

The rule is simple to check: business code is not in the public repo. Billing and customer relations change with how we sell and help customers, and they call vendors (Stripe, Resend, the Claude API) that a self-hosted instance never uses. Keeping them out means self-hosters run only the product, and the product's code can be read without our business tools in the way.

## Considered

- Keep customer relations in the service, off unless set (ADR 0014): rejected because every self-hosted image carried business code it never ran, and customer messages sat in the product's database.
- One private fork of the whole repo: rejected because the open code and the hosted service would drift apart. The hosted image builds on the open image instead, so both run the same product.
