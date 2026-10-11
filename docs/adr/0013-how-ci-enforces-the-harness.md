# How CI enforces the harness and merges green pull requests

This fills in ADR 0003 and ADR 0011 where the code needed details.

- **Lint switches are caught in the source, not only by `forbid`.** `#[tokio::main]` and `#[tokio::test]` write `#[allow(clippy::expect_used)]` into the code they generate, which a `forbid` level refuses to compile. So `expect_used` is `deny` instead of `forbid`, and `scripts/harness_check.py` fails CI on any `allow`, `expect` or `warn` lint attribute in our own Rust code. Every other lint in ADR 0003 stays `forbid`.
- **The harness guard protects what exists.** Changing or deleting a harness file that is already on `main` (the lint block and profiles in `Cargo.toml`, `clippy.toml`, `deny.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.config/nextest.toml`, `.cargo/mutants.toml`, the workflows, the harness scripts, the golden cases) fails CI unless the pull request has the `harness-change` label, and CI checks that the maintainer set it. Adding a new golden case or a new workflow needs no label, so new checks can land while agreed ones cannot be loosened.
- **The test count is counted in the source.** CI counts test functions and golden case folders on the pull request and on its base, and fails if either drops. No build of the base is needed.
- **Green pull requests merge themselves.** When every CI job passes, CI squash-merges the pull request, only if its head is still the commit that was tested, then starts CI on `main` so the image is published. The `hold` label stops this for one pull request.

## Why

A forbid level that breaks the standard async test macro would push every async test into hand-written runtimes; one line of grep keeps the same guarantee. Requiring the label for new files would make every new golden case wait on the maintainer, who does not review code. Counting tests in the source keeps CI fast and still catches deleted tests.
