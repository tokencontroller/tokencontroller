# The web app is a static client of the versioned API

Everything the web app does goes through `/api/v1`, the same API the CLI, the mod and customers use. The service generates the OpenAPI document from the Rust handlers (`utoipa`), and the web app's client and types are generated from it in CI, so a drift between the two fails the build. The web app is a React single-page app in strict TypeScript, built with Vite and served as static files by the service from the same Docker image. Routing and queries are typed (TanStack Router and Query); styling is Tailwind v4 cut down to the design tokens, with copied-in components on Base UI.

## Why

One API for the web app and for customers means the API is complete and tested by the product's own use. Generated clients make the API contract a build check. React and TypeScript have the largest body of examples for agents to work from; the alternatives considered (a server-rendered app, StyleX) were not adopted because they add a second runtime or a second toolchain for no gain at this size.
