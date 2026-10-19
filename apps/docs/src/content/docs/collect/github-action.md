---
title: "CI agents and the GitHub Action"
description: "Report what agents spend in CI with the GitHub Action, which tags that cost gets, and whose messages they are."
---

Some agents run without a person: a PR review bot, a nightly dependency update, a cloud agent. The GitHub Action reports what they spend in GitHub Actions. It installs `tc` on the runner and reports the Claude Code and Codex session files the job wrote. On another CI system, `tc submit` with a service token in `TC_TOKEN` does the same; see [In CI and scripts](/reference/cli#scripts). Other agents report through the [token entries endpoint](/collect/token-entries).

## Set up {#setup}

```yaml
# .github/workflows/review.yml, after the step that runs your agent
- uses: tokencontroller/tokencontroller/packages/github-action@v1
  with:
    project: shop
    feature: ${{ steps.plan.outputs.feature }}  # optional
    tags: purpose=code-review                   # optional, any tags
    token: ${{ secrets.TOKEN_CONTROLLER_TOKEN }}
```

| Input | What it does |
| --- | --- |
| `token` | A service token (`tc_svc_…`), created in the web app under _Settings_. Required. |
| `project` | The project of this job's work. Wins over a rule on the repository. |
| `feature` | The feature of this job's work. Wins over every rule. Without it, `TC_FEATURE` set by an earlier step is used. |
| `tags` | Any other tags, as `name=value`, separated by commas, spaces or new lines. |
| `url` | The address of a self-hosted instance. Leave it out for the hosted service. |

The output `messages` says how many messages the step reported, and the job summary says it in a line.

## Whose messages CI cost is {#ci}

The person who created the service token is the person of every message reported with it, usually the tech lead who set up the agent. Their attributes travel with the messages. How to hand an automation over is under [Grants and roles](/account/roles#person).

## Which tags CI cost gets {#placing}

- A feature given to the Action, or set as `TC_FEATURE` by an earlier step, wins over everything else.
- A review of a pull request lands on the feature of that pull request. The Action reads the pull request's number and title on the runner; only the feature keys found in the title leave it.
- The project named in the Action wins over a rule on the repository.
- CI work that matches no feature lands on the project, as project overhead.

<div class="note"><b>Example.</b> A review bot reviews a pull request for KD-214. The cost shows on KD-214 as direct cost, and its person is the tech lead who runs the bot. The feature's assignee is not charged for it. In the <a href="/concepts#month">worked month</a>, the review bot's session S3 found no feature and sits on Shop as project overhead, with Tom as its person.</div>

The full order is under [Rules](/attribute/rules#order).
