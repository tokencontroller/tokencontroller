# Golden cases

Acceptance cases for the accounting core (cost model 2), one folder per scenario. Each folder is an event log written as CSV files, one file per kind of event, plus the queries to ask of it, and holds the answers the fold must give. `crates/core/tests/golden.rs` runs every folder and compares `answers.csv` exactly. Existing cases only change with the `harness-change` label (ADR 0013); new cases need none.

Each case's `README.md` tells its story with its numbers. `worked-month` is the worked month of the spec. `model-world` is the model world of ADR 0028: three contracts, four bills and every hard case of the data model.

## Inputs: the event log

Every event file has `id` and `recorded_at` (RFC 3339, UTC); `splits.csv` also has `supersedes`. A file that a case does not need is left out. Prices and amounts are in USD. Tags are `name=value` pairs joined by `;`. Dates are `YYYY-MM-DD`.

| File | Columns | Notes |
| --- | --- | --- |
| `tag_values.csv` | `name,key,title,source_status,defaults` | `defaults` are the tags the value brings, such as `project=shop` |
| `attribute_values.csv` | `name,key,defaults` | Attributes of people, such as team `web` with default `department=engineering` |
| `person_attributes.csv` | `person,name,key` | Puts an attribute value on a person |
| `contracts.csv` | `key,provider,kind,fee` | A value of the tag `contract` with its terms. `kind` is `metered` or `flat`; `fee` is per month and may be empty |
| `prices.csv` | `provider,model,kind,per_million,valid_from,source`, optional `contract` | `kind` is any token kind, such as `input` or `cache_write_1h`; `source` is `managed` or `organization`; a `contract` makes the row that contract's own price |
| `messages.csv` | `session,message,person,provider,model,at,input,output,cache_write,cache_read,agent_harness,agent_name,reported_cost,tags,branch,prompt_keys`, optional `cache_write_5m,cache_write_1h,kinds,resource` | All counts empty means no counts (a count error). An empty `provider` means the sender did not report one. `kinds` are other token kinds as `name=count` joined by `;`; `resource` are telemetry resource attributes the same way. `prompt_keys` are joined by spaces. A message id starting with `otel:` was built by the usage endpoint |
| `contract_assignments.csv` | `session,message,contract` | A contract set by hand; an empty `message` sets the whole session |
| `splits.csv` | `supersedes,session,message,parts` | An empty `message` splits the session. `parts` is like `60 feature=KD-1\|40 feature=KD-2` |
| `bills.csv` | `provider,from,to,amount`, optional `contract,contract_kind,fee,extra_usage` | Without a `contract` the bill goes to the provider's only contract, else to one named after the provider |
| `estimates.csv` | `description,tags,median,markup_basis_points,estimator,from,to` | The distribution's median; `from` and `to` are the days the plan spreads over |
| `accepts.csv` | `estimate` | The id of the estimate's event |
| `shares.csv` | `person,project,fraction_basis_points,from,to` | `to` empty is open-ended |
| `grants.csv` | `person,capability,filter` | `capability` is `view`, `assign`, `approve`, `plan`, `grant`, `adjust` or `cutoff` |
| `rules.csv` | `name,scope,order,signal,pattern,then`, optional `creates` | `scope` is `organization` or a person. `signal` is `branch`, `repository`, `pull_request_title`, `prompt_keys`, `agent_name`, `config NAME` or `resource NAME`. `then` is `key_of feature` or `tag project=shop`. `creates` is the contract the rule creates on its first match, as `provider kind fee`, such as `anthropic flat 150` |
| `cutoffs.csv` | `date` | |
| `submits.csv` | `person,from,to` | |
| `approvals.csv` | `submission,decision,reason` | `decision` is `approved` or `rejected` |
| `reversals.csv` | `of` | The id of the event reversed |

## Queries

`queries.csv` has `as_of,kind,filter,from,to,argument`. Each row folds the log as of `as_of` and asks one question; `filter` is tags and attributes joined by `;`, `from` and `to` are the window.

| Kind | Answers | `argument` |
| --- | --- | --- |
| `messages` | `C(e)` of every counting message: key `session/message`, amount, note `priced`, `count error` or `not priced yet`, with each unpriced kind | |
| `contracts` | Every counting message's contract, how it got it, and the provider it is priced under | |
| `matched` | Every message with a built id that stopped counting, and the message it counts as | |
| `lines` | Every split part: key `session/message#part`, amount, note `assigned by; label; closed tags` | |
| `cost` | `C(F, w)`, with a note naming count errors, messages not priced yet with their counts, and unpriced kinds | |
| `by_contract_kind` | `C(F, w)` by `metered`, `flat` (on a plan) and `none` | |
| `by` | `C(F, w)` partitioned by one tag name, an empty key for lines without it | the tag name |
| `by_label` | `C(F, w)` by `direct`, `project overhead`, `organization overhead` | |
| `reconcile` | `bill`, `messages` and `unexplained` (R); note `a message is wrong` when R < 0 | the bill's event id |
| `check` | The bill's contract and kind, `bill`, `messages`, `unexplained`, for a flat contract `fee`, `extra usage` and `covered by the plan`, then `same provider no contract` and `contract from fallback` | the bill's event id |
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
