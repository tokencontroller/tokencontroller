# Token Controller GitHub Action

Reports what agents spend in CI, such as a pull request review bot, to [Token Controller](https://tokencontroller.com). It wraps the `tc` command: it installs `tc` 1.0.0 for the runner, reads the Claude Code and Codex session files the job's agents wrote, and reports every message with the job's project, feature and pull request.

```yaml
# .github/workflows/review.yml, after the step that runs your agent
- uses: tokencontroller/tokencontroller/packages/github-action@v1
  with:
    project: shop
    feature: ${{ steps.plan.outputs.feature }}  # optional
    tags: purpose=code-review                   # optional, any tags
    token: ${{ secrets.TOKEN_CONTROLLER_TOKEN }}
```

| Input | |
| --- | --- |
| `token` | A service token (`tc_svc_…`) from the web app, under _Settings_. Its creator is the person of every message the step reports. Required. |
| `project` | The project of this job's work. Wins over a rule on the repository. |
| `feature` | The feature of this job's work. Wins over every rule. Without it, `TC_FEATURE` set by an earlier step is used. |
| `tags` | Any other tags as `name=value`, separated by commas, spaces or new lines. |
| `url` | The address of a self-hosted instance. Leave it out for the hosted service. |

The output `messages` says how many messages the step reported, and the job summary says it in a line, such as `Reported 2 messages on KD-214 · shop, 3.20 USD.`

A review of a pull request lands on the feature of that pull request: the Action hands `tc` the pull request's number and title on the runner, and only the feature keys found in the title leave it. CI work that matches no feature lands on the project, as project overhead. Messages from the Action count in reports on arrival. More in the docs: [CI agents and the GitHub Action](https://docs.tokencontroller.com/collect/github-action).

## Working on it

GitHub runs `dist/`, so `dist/` is committed. After changing `src/`, run `pnpm build` here and commit `dist/`; a test fails while the two differ. MIT license.
