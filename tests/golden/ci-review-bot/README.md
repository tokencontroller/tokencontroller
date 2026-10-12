# CI review bot

From the docs pages [CI agents](https://docs.tokencontroller.com/attribute/ci) and [GitHub Action](https://docs.tokencontroller.com/collect/github-action).

Tom, a tech lead at Kestrel Digital, created the service token the GitHub Action reports with, so he is the agent manager of everything it reports. The Action sends `purpose=code-review` with its reviews. Everything it reports arrives automatically, submitted.

| # | What | Placed |
| --- | --- | --- |
| 1 | Mara fixes KD-214 on her laptop | KD-214, by the key in her branch |
| 2 | The review bot reviews pull request 412, titled for KD-214 | KD-214, by the key in the pull request title. The cost sits with Tom, not Mara |
| 3 | The review bot runs with `ticket: KD-240` given to the Action, on a pull request titled for KD-214 | KD-240: a ticket given to the Action wins over every rule |
| 4 | A nightly dependency update with no ticket | the project storefront, by its repository |
| 5 | A job in the storefront repository whose Action names `project: intranet` | the project intranet: the named project wins over the repository |

The October bill is 60.00 USD; the entries explain 49.42, so 10.58 is residual.

What it checks: who answers for CI cost, the pull request title rule, tags sent by the Action, and the order of named ticket, rules, named project and repository. The cost sheet shows `person` and `purpose` as extra columns, so the review cost on KD-214 is visible next to Mara's own work on it.
