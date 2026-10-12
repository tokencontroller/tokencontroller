# Token Controller

Cost accounting and management control for AI agents.

The docs at [docs.tokencontroller.com](https://docs.tokencontroller.com) say exactly how Token Controller works. They are written first, and the code is built to match them.

## Layout

| Folder | What it holds | License |
|---|---|---|
| `apps/docs/` | The docs, built with Astro and Starlight: one Markdown file per page in `src/content/docs/`, and the glossary in `glossary.md` | |
| `docs/adr/` | Architecture decision records: how the code is built and why | |
| `apps/server/` | The service at app.tokencontroller.com, in Rust: sign-in, API, telemetry ingest, background jobs, MCP server. It also serves the web app | AGPL-3.0 |
| `apps/web/` | The web app, built into the service's Docker image | AGPL-3.0 |
| `apps/cli/` | The `tc` command, in Rust, with its reference classifier script and the release packaging. A `v*` tag runs `.github/workflows/release.yml` | MIT |
| `crates/` | Rust libraries the service and the CLI share: `core` holds money, rules and the `Extension` trait | AGPL-3.0 or MIT, per crate; `core` is MIT |
| `packages/` | Everything published to a registry, one folder each: `claude-code-mod` first, then one mod per agent harness that allows mods, `sdk` and `github-action` | MIT |
| `Formula/` | The Homebrew formula for `tc`, so `brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller` works. The release workflow writes it | MIT |
| `.claude-plugin/` | The plugin marketplace Claude Code reads with `/plugin marketplace add tokencontroller/tokencontroller`. It lists the mod in `packages/claude-code-mod` | |
| `tests/golden/` | Acceptance cases as CSV, one folder per scenario in the docs | |
| `scripts/` | The harness check and the end-to-end smoke test CI runs | |

Rust code is one Cargo workspace (`Cargo.toml` at the root). TypeScript code is one pnpm workspace (`pnpm-workspace.yaml` at the root). The `Dockerfile` for the service image and `compose.yaml`, the self-hosting compose file from the docs, sit at the root too.

### The service, by area

The service has one module folder per area under `apps/server/src/`, so work on different areas never touches the same files. Each folder registers its own routes; `api.rs` only puts them together.

| Module | Area |
|---|---|
| `status` | `/api/v1/health` and the status signals status.tokencontroller.com reads |
| `organizations` | Organizations, members, invites, roles, organization settings |
| `sign_in` | Magic links, sessions, API tokens |
| `tags` | Tags and the tag map |
| `projects` | Projects, clients and tickets |
| `mcp` | The MCP server |
| `seed` | The example organization for local runs, CI, staging and screenshots |
| `ingest` | Telemetry, the token entries endpoint, manual entry, CSV import, dedupe per message |
| `jobs` | The job runner: advisory locks and heartbeats |
| `integrations` | Bill pulls, ticket sync, the price table refresh, reminders |
| `token_sheets` | Review, submit, cadence, approval |
| `bills` | Bills and reconciliation |
| `close` | The monthly close, adjustments, the cost sheet |
| `budgets` | Token budgets and token forecasts |
| `reports` | Reports, CSV exports, the reporting API, the raw export |
| `billing` | Team plan subscriptions on the hosted service, off unless Stripe is configured |
| `customer_relations` | Feedback, the newsletter, the support inbox |

## Working on the code

- `cargo nextest run` runs every test. Tests that touch the database need Postgres 15 or newer at `DATABASE_URL`; each test gets its own fresh database.
- `cargo run -p tokencontroller-server -- seed` (or `docker run … seed` on the image) applies migrations and loads the example organization into `DATABASE_URL`. Staging runs it after every reset.
- Migrations live in `apps/server/migrations/`, named `<UTC timestamp>_<module>_<what>.sql`. The service applies them when it starts. A migration that reached `main` is never edited.
- Queries are checked against the database when the code compiles. After adding or changing a query, run `cargo sqlx prepare --workspace` and commit `.sqlx/`, so the image builds without a database.
- After changing the API, run `cargo run -p tokencontroller-server -- openapi > apps/server/openapi.json` and commit it. The web app's client is generated from this file.
- CI fails on any warning, a skipped test, a lint switched off in the code, fewer tests than before, or a changed harness file without the maintainer's label (ADR 0003, ADR 0013). A pull request merges itself when CI is green, unless it carries the `hold` label.

## Open core

Everything here is open source. The Enterprise plan features are kept in a private repo that builds on this one; this repo never depends on it. What that means for customers is on the docs pages [Enterprise plan instances](https://docs.tokencontroller.com/self-hosting/enterprise) and [Open source and licenses](https://docs.tokencontroller.com/reference/open-source).
