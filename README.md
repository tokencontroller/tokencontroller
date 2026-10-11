# Token Controller

Cost accounting and management control for AI agents.

The docs at [docs.tokencontroller.com](https://docs.tokencontroller.com) say exactly how Token Controller works. They are written first, and the code is built to match them.

## Layout

| Folder | What it holds | License |
|---|---|---|
| `apps/docs/` | The docs: page sources, the glossary, the build script and the built site | |
| `docs/adr/` | Architecture decision records: how the code is built and why | |
| `apps/server/` | The service at app.tokencontroller.com, in Rust: sign-in, API, telemetry ingest, background jobs, MCP server. It also serves the web app | AGPL-3.0 |
| `apps/web/` | The web app, built into the service's Docker image | AGPL-3.0 |
| `apps/cli/` | The `tc` command, in Rust | MIT |
| `crates/` | Rust libraries the service and the CLI share, such as `core` (money and rules) | AGPL-3.0 or MIT, per crate |
| `packages/` | Everything published to a registry, one folder each: `claude-code-mod` first, then one mod per agent harness that allows mods, `sdk` and `github-action` | MIT |

Rust code is one Cargo workspace (`Cargo.toml` at the root). TypeScript code is one pnpm workspace (`pnpm-workspace.yaml` at the root). The `Dockerfile` for the service image sits at the root too. Only `apps/docs/` and `docs/adr/` exist so far; the rest arrives with the code.

## Open core

Everything here is open source. The Enterprise plan features are kept in a private repo that builds on this one; this repo never depends on it. What that means for customers is on the docs pages [Enterprise plan instances](https://docs.tokencontroller.com/self-hosting/enterprise) and [Open source and licenses](https://docs.tokencontroller.com/reference/open-source).
