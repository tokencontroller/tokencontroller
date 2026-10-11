# This repo builds and publishes the image; deployment lives in the private repo

CI here runs on every push and pull request without secrets: format, lint, the harness checks, unit and golden tests, an image build, and end-to-end tests against the image started with the self-hosting compose file. A push to the default branch publishes the image to `ghcr.io/tokencontroller/tokencontroller` tagged with the commit; a release tag adds the version tags (`1.0.0`, `1.0`). Nothing here deploys anywhere. The private repo deploys the published image to the hosted service and to Enterprise plan instances, and holds the infrastructure files and secrets.

Pull requests merge automatically once CI is green.

## Why

Secrets must not reach workflows that outside pull requests can trigger, and deployment details are not open source. Testing against the self-hosting compose file keeps the self-hosting docs true on every push. Version tags on the image are what the docs tell self-hosters to pull.
