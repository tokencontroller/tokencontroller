# Review and submit

From the docs pages [Quick start](https://docs.tokencontroller.com/#review) and [Review and submit](https://docs.tokencontroller.com/attribute/review).

Mara, at Kestrel Digital, runs five Claude Code sessions on 14 October. Rules place four of them:

| # | Cost | Placed by |
| --- | --- | --- |
| 1 | 41.87 | the ticket key KD-214 in the branch |
| 2 | 12.10 | the same branch |
| 3 | 88.30 | the ticket key KD-231 in the pull request title |
| 4 | 9.75 | the repository, mapped to the project storefront |
| 5 | 60.38 | nothing: no key, and a repository no project maps |

She fixes the exceptions before submitting:

```sh
tc tag 5 ticket=KD-231
tc tag 4 purpose=learning
tc split 3 KD-231=60 KD-240=40
```

Her token sheet comes to 212.40 USD, 202.65 of it on tickets (95%). The October Console bill is 250.00 USD, so 37.60 is residual.

What it checks: the default rules in their order, a named ticket bringing its project, a split by hand, a person's team inherited onto every split, and project overhead for a repository match.
