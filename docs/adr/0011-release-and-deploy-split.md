# This repo builds and publishes the image; deployment lives in the private repo

CI here runs without secrets, in tiers so nothing runs twice for the same code. A pull request runs the fast checks: format, lint, the harness checks, unit and golden tests, dependencies, mutation testing of changed core code. The merge queue runs the harness, Rust and TypeScript checks again together with what is ahead in the queue, plus an image build and end-to-end tests against the image started with the self-hosting compose file. A push to the default branch only builds and publishes the image to `ghcr.io/tokencontroller/tokencontroller` tagged with the commit; a release tag adds the version tags (`1.0.0`, `1.0`). Nothing here deploys anywhere. The private repo builds the hosted and Enterprise images on top of the published image, deploys them to the hosted service and to Enterprise plan instances, and holds the infrastructure files and secrets.

Pull requests merge automatically once CI is green.

## Why

Secrets must not reach workflows that outside pull requests can trigger, and deployment details are not open source. Testing against the self-hosting compose file in the queue keeps the self-hosting docs true for every change that lands. Version tags on the image are what the docs tell self-hosters to pull.
