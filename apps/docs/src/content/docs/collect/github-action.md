---
title: "CI agents and the GitHub Action"
description: "Report what agents spend in CI with the GitHub Action, how that cost is placed, and who answers for it."
---

Some agents run without a person: a PR review bot, a nightly dependency update, a cloud agent. The GitHub Action reports what they spend in GitHub Actions. It installs `tc` on the runner and reports the Claude Code and Codex session files the job wrote. On another CI system, `tc submit` with a service token in `TC_TOKEN` does the same; see [In CI and scripts](/reference/cli#scripts). Other agents report through the [token entries endpoint](/collect/token-entries).

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

| Input | What it does |
| --- | --- |
| `token` | A service token (`tc_svc_…`), created in the web app under _Settings_. Required. |
| `project` | The project of this job's work. Wins over matching by repository. |
| `ticket` | The ticket of this job's work. Wins over every rule. Without it, `TC_TICKET` set by an earlier step is used. |
| `tags` | Any other tags from your organization's list, as `name=value`, separated by commas, spaces or new lines. |
| `url` | The address of a self-hosted instance. Leave it out for the hosted service. |

The output `token-entries` says how many token entries the step reported, and the job summary says it in a line.

## Who answers for CI cost {#ci}

The person who creates the service token is the agent manager of everything reported with it, usually the tech lead who set up the agent. The token entries are [automatic](/collect#automatic). How to hand an automation over is under [Roles](/account/roles).

## How CI cost is placed {#placing}

- A ticket given to the Action, or set as `TC_TICKET` by an earlier step, wins over everything else.
- A review of a pull request lands on the ticket of that pull request. The Action reads the pull request's number and title on the runner; only the ticket keys found in the title leave it.
- The project named in the Action wins over matching by repository.
- CI work that matches no ticket lands on the project.

<div class="note"><b>Example.</b> A review bot reviews a pull request for KD-214. The cost shows on KD-214 as review cost, and sits with the tech lead who runs the bot. The ticket's assignee is not charged for it.</div>

The full order is under [Rules](/attribute/rules#order).
