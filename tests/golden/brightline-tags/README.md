# Brightline: tags, the tag map and reports

The worked example behind the docs page [Tags and the tag map](https://docs.tokencontroller.com/account/tags), with the docs' current terms: learning is tagged `purpose: learning` and is organization overhead like any cost without a project. Every token entry has a person, so all of it is attributed; only residual is not.

**Brightline** is an agency with three teams (web, mobile, data), two clients (Acme and Globex) and internal work. Its controller sets:

| Setting | Brightline's choice |
| --- | --- |
| Tag names | `client`, `team`, `purpose` (plus the built-in `person`, `ticket`, `project`) |
| Cost center | `team` |
| Ladder | ticket, project, client, organization |
| From the person | `team` |
| From the project | `client` |

In October, Anthropic bills 2,000 USD of metered usage.

## Five token entries

| # | What happened | Ends up as |
| --- | --- | --- |
| 1 | Mara (web) fixes a checkout bug; the key ACME-42 is in her branch | ACME-42, acme-shop, acme, web: 18 |
| 2 | Tom's (mobile) review bot reviews a Globex pull request; the GitHub Action sends `project: globex-app` and `purpose: code-review` | globex-app, globex, mobile, project overhead: 6 |
| 3 | Lena (data) works on two tickets in one session and splits it 60/40 | ACME-50 6 and GLOBEX-7 4, both data |
| 4 | Jonas (web) tries out a new framework and tags it `purpose: learning` | web, organization overhead: 3 |
| 5 | Sam (mobile) has no mod; only telemetry arrives. His person record has no team yet, but the company's managed settings add `team=mobile` to all telemetry | mobile, organization overhead: 2 |

Entries 6 to 21 stand for the rest of October, one row per person and kind of work. Entry 8 names ACME-44 twice and ACME-45 once in its prompts, so ACME-44 wins.

## The month in numbers

Token entries come to 1,880 USD, so 120 USD of the bill is residual.

| By project | Cost |
| --- | --- |
| acme-shop (client acme) | 900 |
| globex-app (client globex) | 520 |
| intranet | 120 |
| No project: organization overhead, 60 of it learning | 340 |
| **Token entries** | **1,880** |
| Residual | 120 |
| **Bill** | **2,000** |

By cost center: web 820, mobile 640, data 420.

Reports: `client: acme` by team is web 600 and data 300. `purpose: code-review` is 140, 60 on Acme and 80 on Globex. `team: web` by project is acme-shop 600, intranet 80, and 140 without a project.

Token budgets: acme-shop has 1,000 and spent 900. Team web has 800 and spent 820, an overrun of 20. Both contain Mara's 18: each budget is its own view, and the two are never added up.

What it checks: a custom cost center and a longer ladder, tags inherited from person and project, telemetry tags filling a missing team, a split by hand, the "most often" choice among prompt keys, the three cost kinds, residual, reports that add up per tag name, and token budgets with overlapping filters.
