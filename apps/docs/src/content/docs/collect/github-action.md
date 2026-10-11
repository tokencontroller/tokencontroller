---
title: "GitHub Action"
description: "Report what agents spend in CI with the GitHub Action."
---

The GitHub Action reports what agents spend in CI, such as a PR review bot. It wraps the CLI.

## Set up {#setup}

```yaml
# .github/workflows/review.yml, after the step that runs your agent
- uses: tokencontroller/tokencontroller/packages/github-action@v1
  with:
    project: storefront
    ticket: ${{ steps.plan.outputs.ticket }}  # optional
    tags: purpose=code-review                 # optional, any tags
    token: ${{ secrets.TOKEN_CONTROLLER_TOKEN }}
```

Create the service token in the web app under _Settings_. The person who creates it becomes the agent manager of everything the Action reports.

## How CI cost is placed {#placing}

When the job knows its ticket, pass it as `ticket`, or set `TC_TICKET` in an earlier step. That ticket wins over every rule. The Action names the project, and that wins over matching by repository. A review of a pull request is placed on the ticket of that pull request. CI work that matches no ticket falls back to the project. See [CI agents](/attribute/ci).

## No review step {#automatic}

Token entries from the Action arrive automatically, with the status submitted. They never go on a token sheet.
