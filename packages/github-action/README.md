# Token Controller GitHub Action

Reports what agents spend in CI, such as a pull request review bot, to [Token Controller](https://tokencontroller.com). It wraps the `tc` command: it installs `tc` 1.0.0 for the runner, reads the session files the job's agents wrote, and reports them with the job's project, ticket and pull request.

```yaml
# .github/workflows/review.yml, after the step that runs your agent
- uses: tokencontroller/tokencontroller/packages/github-action@v1
  with:
    project: storefront
    ticket: ${{ steps.plan.outputs.ticket }}  # optional
    tags: purpose=code-review                 # optional, any tags
    token: ${{ secrets.TOKEN_CONTROLLER_TOKEN }}
```

| Input | |
| --- | --- |
| `token` | A service token (`tc_svc_…`) from the web app, under _Settings_. Its creator is the agent manager of everything the step reports. Required. |
| `project` | The project. Wins over matching by repository. |
| `ticket` | The ticket. Wins over every rule. Without it, `TC_TICKET` set by an earlier step is used. |
| `tags` | Any other tags as `name=value`, separated by commas, spaces or new lines. |
| `url` | The address of a self-hosted instance. Leave it out for the hosted service. |

The output `token-entries` says how many token entries the step reported, and the job summary says it in a line.

A review of a pull request is placed on the ticket of that pull request: the Action passes the pull request's number and title, where ticket keys are found. Token entries from the Action arrive submitted and never go on a token sheet. More in the docs: [GitHub Action](https://docs.tokencontroller.com/collect/github-action) and [CI agents](https://docs.tokencontroller.com/attribute/ci).

## Working on it

GitHub runs `dist/`, so `dist/` is committed. After changing `src/`, run `pnpm build` here and commit `dist/`; a test fails while the two differ. MIT license.
