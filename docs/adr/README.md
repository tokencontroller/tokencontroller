# Architecture decision records

One file per decision, numbered in the order they were made. Each one says what was decided and why, in a few sentences. The docs at docs.tokencontroller.com say what the product does; these records say how it is built and why. A record never contradicts the docs. When a decision is replaced, its record is deleted in the same change that replaces it, and the new record carries the next free number; the git history keeps the old one.

| | Decision |
| --- | --- |
| 0001 | Open core: one public repo, closed features in a private one |
| 0002 | One service, one image, one database |
| 0003 | Strict Rust harness |
| 0004 | Money is an integer number of nano-units |
| 0005 | Postgres with plain SQL |
| 0006 | Cost is stored per message |
| 0009 | API-first web app |
| 0010 | Local tools share the CLI |
| 0011 | Release and deploy are split |
| 0012 | Docs on Astro Starlight |
| 0013 | How CI enforces the harness |
| 0014 | Support and newsletter in the service |
| 0017 | How `tc` reads sessions, splits them, signs in and ships |
| 0018 | Doors join per session and message |
| 0020 | The public repo holds the product only |
| 0021 | A model call that carries prompt text keeps its counts and loses the text |
| 0022 | The organization is an append-only event log; figures are folds as of a date |
| 0023 | One filter formula over tags and attributes is the only report and access primitive |
| 0024 | The price table is the only cost basis; reported cost is a fact, never a figure |
| 0025 | Agent formats are checked against the real CLIs |

The Enterprise plan features and our own billing and support tools are built in a private repo (ADR 0020). Decisions about how they plug in and how instances are run are recorded there.
