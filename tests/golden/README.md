# Golden cases

Acceptance cases for the accounting core (cost model 2), one folder per scenario. Each folder is an event log written as CSV files, one file per kind of event, plus the queries to ask of it, and holds the answers the fold must give. `crates/core/tests/golden.rs` runs every folder and compares `answers.csv` exactly. Existing cases only change with the maintainer's `harness-change` label (ADR 0013); new cases need none.

Each case's `README.md` tells its story with its numbers. `worked-month` is the worked month of the spec.

## Inputs: the event log

Every event file has `id` and `recorded_at` (RFC 3339, UTC); `splits.csv` also has `supersedes`. A file that a case does not need is left out. Prices and amounts are in USD. Tags are `name=value` pairs joined by `;`. Dates are `YYYY-MM-DD`.

| File | Columns | Notes |
| --- | --- | --- |
| `tag_values.csv` | `name,key,title,source_status,defaults` | `defaults` are the tags the value brings, such as `project=shop` |
| `attribute_values.csv` | `name,key,defaults` | Attributes of people, such as team `web` with default `department=engineering` |
| `person_attributes.csv` | `person,name,key` | Puts an attribute value on a person |
| `prices.csv` | `provider,model,kind,per_million,valid_from,source` | `kind` is `input`, `output`, `cache_write` or `cache_read`; `source` is `managed` or `organization` |
| `messages.csv` | `session,message,person,provider,model,at,input,output,cache_write,cache_read,agent_harness,agent_name,reported_cost,tags,branch,prompt_keys` | All four counts empty means no counts (a count error). `prompt_keys` are joined by spaces |
| `splits.csv` | `supersedes,session,message,parts` | An empty `message` splits the session. `parts` is like `60 feature=KD-1\|40 feature=KD-2` |
| `bills.csv` | `provider,from,to,amount` | |
| `estimates.csv` | `description,tags,median,markup_basis_points,estimator,from,to` | The distribution's median; `from` and `to` are the days the plan spreads over |
| `accepts.csv` | `estimate` | The id of the estimate's event |
| `shares.csv` | `person,project,fraction_basis_points,from,to` | `to` empty is open-ended |
| `grants.csv` | `person,capability,filter` | `capability` is `view`, `assign`, `approve`, `plan`, `grant`, `adjust` or `cutoff` |
| `rules.csv` | `name,scope,order,signal,pattern,then` | `scope` is `organization` or a person. `signal` is `branch`, `repository`, `pull_request_title`, `prompt_keys`, `agent_name`, `config NAME` or `resource NAME`. `then` is `key_of feature` or `tag project=shop` |
| `cutoffs.csv` | `date` | |
| `submits.csv` | `person,from,to` | |
| `approvals.csv` | `submission,decision,reason` | `decision` is `approved` or `rejected` |
| `reversals.csv` | `of` | The id of the event reversed |

## Queries

`queries.csv` has `as_of,kind,filter,from,to,argument`. Each row folds the log as of `as_of` and asks one question; `filter` is tags and attributes joined by `;`, `from` and `to` are the window.

| Kind | Answers | `argument` |
| --- | --- | --- |
| `messages` | `C(e)` of every message: key `session/message`, amount, note `priced`, `count error` or `unknown model` | |
| `lines` | Every split part: key `session/message#part`, amount, note `assigned by; label; closed tags` | |
| `cost` | `C(F, w)`, with a note naming count errors and unknown models | |
| `by` | `C(F, w)` partitioned by one tag name, an empty key for lines without it | the tag name |
| `by_label` | `C(F, w)` by `direct`, `project overhead`, `organization overhead` | |
| `reconcile` | `bill`, `messages` and `unexplained` (R); note `a message is wrong` when R < 0 | the bill's event id |
| `plan` | `P(F, w)`, or key `none` | |
| `deviation` | `plan`, `actual` and `delta`; note `favourable` or `unfavourable` | |
| `forecast` | The forecast for the window from the past windows | past windows `from..to;from..to` |
| `can` | `true` or `false` | `person capability` |
| `share_sums` | Each project's share fractions in basis points on `from`; note `over 1` | |
| `submissions` | Every submission with its status | |
| `cutoff` | The cutoff date or `none`; note the frozen as-of moment | |
| `history` | `H`: every tag value with its cost so far and last activity | |

## Expected output

`answers.csv`: `as_of,kind,filter,from,to,argument,key,amount,note`, one line per answer, in the order of the queries. Amounts carry every digit they have, at least to the cent.

The runner writes every case's computed answers to `target/tmp/golden/<case>/answers.csv`. To add a case, write its inputs and queries, work out the expected numbers by hand from the spec or the docs, and compare them with that file before copying it in.
