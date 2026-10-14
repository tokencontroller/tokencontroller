# Open core: one public repo, enterprise features and business code in a private repo

The service, the web app, the CLI, the mods, the GitHub Action, the SDK and the docs live in this public repo. The service and the shared crates are AGPL-3.0; the local tools are MIT, so companies can use them in closed tooling. The Enterprise plan features (OIDC and SCIM, audit log, telemetry passthrough, cloud bill connectors) live in a private repo that depends on this one. So does our own business code, Team plan billing and customer relations (ADR 0020). This repo never depends on the private one, and nothing in it is hidden: no private branches, no feature flags for closed features. Everything also ships from this one repo: the plugin marketplace for the mods, the GitHub Action (`packages/github-action`, used as `tokencontroller/tokencontroller/packages/github-action@v1`), the Homebrew formula for `tc` and the release binaries. There are no satellite repos.

## The plug-in points

The `core` crate defines one Rust trait, `Extension`, with a default implementation that does nothing. The service calls it at a fixed, small set of hooks that need no database: sign-in providers, audit events, telemetry forwarding, bill connectors. Hooks that need the database sit in the service's `ServiceHooks` trait (`apps/server/src/hooks.rs`), also with defaults that do nothing. A build adds its own routes with `serve_with_additions`, and its own web app pages through the web app's `src/additions/` slot. The open-source binary uses the defaults, so the open image carries no private code. The enterprise binary links its own implementation at build time as a normal Cargo dependency. There is no runtime plug-in loading. How the enterprise build is unlocked and delivered is recorded in the private repo.

## Why

Traits with defaults keep the seams in a few places a reader can find, and keep the open code honest: it cannot call anything that is not in this repo. Build-time linking avoids a plug-in loader and keeps one Docker image per edition, each built on the open one. Pull requests from outside are not accepted, so one person holds the copyright and can combine the AGPL code with closed code without a contributor agreement.

## Considered

- Feature flags in one repo with a closed build: rejected because the closed code would be visible in the public history.
- Runtime plug-ins loaded from a shared library: rejected because it adds an unsafe boundary and a second packaging format for no gain while there is one enterprise crate.
