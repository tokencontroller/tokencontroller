# The CLI owns local state and the protocol; the mod and the Action wrap it

The CLI, `tc`, is the one local program: it reads session files through the ccusage adapter crates, keeps draft token entries in a local SQLite file, runs the rules, calls the optional classifier, and talks to the service over the documented HTTP API. The Claude Code mod (TypeScript) and the GitHub Action do not implement any of this again: they call `tc` for state, rules and submission, and add only what they alone can see (the mod: Claude Code's own usage per turn and the summary it writes; the Action: the job's project and ticket). The rules engine lives in the `core` crate and is the same code on the machine and in the service, so both passes place a token entry the same way.

There is no background process and no sync between a person's machines; each machine submits its own token entries.

ADR 0015 replaces the ccusage adapter crates with readers of our own that follow ccusage's rules, and fills in the rest of how `tc` works.

## Why

One implementation of rules and local state means one set of golden tests and no drift between what the mod shows and what the service computes. The CLI stays fully useful with no service connected, as the docs promise, and any compatible service can be connected because the protocol is the public API.
