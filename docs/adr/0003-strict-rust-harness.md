# Rust everywhere on the server side, with a strict harness

The service, the CLI and the shared libraries are one Cargo workspace. Rust is written by AI agents and reviewed by tests, so the workspace turns the strictest settings on and CI fails on any warning: `unsafe_code` forbidden, Clippy `all` and `pedantic` denied, `unwrap`, `expect`, `panic`, `todo`, indexing, float arithmetic, unchecked arithmetic and `as` casts forbidden, `#[allow]` forbidden, overflow checks on in release. Tests may use `unwrap` and friends through `clippy.toml`. The toolchain is pinned in `rust-toolchain.toml`. `cargo-deny` checks advisories, licenses and sources. Tests run with `cargo nextest`; CI fails if a test is `#[ignore]`d or the test count drops.

The harness files (the lint block in `Cargo.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`, the workflows and the golden test data) are owned by the maintainer: a change to them needs a pull request label that only the maintainer sets.

Acceptance tests are data: a folder per case under `tests/golden/` with messages, a price table, a bill, rules and the expected figures (cost per filter, reconciliation, plan, deviation) as CSV, one case per scenario in the docs. The money core also gets property tests (`proptest`) and nightly mutation testing (`cargo-mutants`).

## Why

A green CI run has to mean the code can be trusted with money without a human reading the Rust. Forbid-level lints cannot be switched off inside the code, so the usual ways an agent makes a test pass are closed. Golden cases are spreadsheets the maintainer can read.
