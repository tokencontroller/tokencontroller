# Golden cases

Acceptance cases for the accounting core, one folder per scenario in the docs. Each folder describes an organization, its token entries and its bills as CSV, and holds what the core must make of them. `crates/core/tests/golden.rs` runs every folder and compares its outputs with the expected files exactly. Existing cases only change with the maintainer's `harness-change` label (ADR 0013); new cases need none.

Each case's `README.md` tells its story and links the docs pages it comes from.

## Inputs

| File | Columns | Notes |
| --- | --- | --- |
| `organization.csv` | `setting,value` | `reporting_currency`, `cost_source`, `period` (required); `approval` (`true`), `tag_names`, `cost_center`, `ladder` (most specific first), `several_keys`, `missing_tickets`, `columns` (extra cost sheet columns), `price_version` |
| `people.csv` | `person,tags` | |
| `projects.csv` | `project,ticket_key_pattern,repositories,tags` | |
| `tickets.csv` | `ticket,project,parent,title,tags` | |
| `rules.csv` | `owner,name,when,then` | Added after the default rules. `owner` is `organization` or a person. `when`: conditions such as `branch starts_with review/`, joined by `;`. `then`: `set name=value;...`, `ticket_key_in branch`, or `repository_project` |
| `prices.csv` | `version,model,input,output,cache_write,cache_read` | USD per million tokens. Each version changes the one before it |
| `token_entries.csv` | `entry,person,agent_harness,agent_name,started_at,status,arrival,bill,tags,configured,signals,summary,hand_split` | `person` is the agent manager. `bill` names a row of `bills.csv`. `tags` are named by hand or sent with the entry; `configured` come from a config file or telemetry. `hand_split` is like `60 ticket=A-1\|40 ticket=B-2` |
| `messages.csv` | `entry,message,at,model,input,output,cache_write,cache_read,reported_cost,billed_cost,signals` | `model` is `provider/model`. Rows without a `message` are an entry's totals; rows with one are its messages, cut into segments |
| `bills.csv` | `bill,provider,cost_source,period,currency,gross,discount,credit,net,rate,revision` | `rate` converts into the reporting currency. Token entries on no bill, such as usage on a seat plan, leave `bill` empty: they count in reports but are not on the cost sheet |
| `adjustments.csv` | `tags,amount,reason,posted_by,posted_at,late` | Optional |

Lists inside a cell are joined by `;`, tags are `name=value`, and times are RFC 3339 in UTC. `signals` are `name=value` pairs of `branch`, `working_directory`, `repository`, `pull_request`, `subagent_type`, `prompt_keys`, `session_title_keys` and `pull_request_title_keys`; key lists are joined by spaces.

## Expected outputs

| File | What |
| --- | --- |
| `splits.csv` | Every token entry's splits: tags, exact list value, how it was placed, and notes (ticket not synced, unknown model, dropped tag names) |
| `cost_sheet.csv` | The cost sheet CSV of `period`, exactly as the core writes it |
| `reports.csv` | Optional. For each `filter` and `group_by` it names, what the splits that count in reports come to per value; an empty `group_by` gives the filter's total |
| `budgets.csv` | Optional. Each filter's token budget next to its actual cost |
| `close.csv` | Optional. The close check of `period` |

The runner writes every output it computes to `target/tmp/golden/<case>/`. To add a case, write its inputs, work out the expected numbers by hand from the docs scenario, and compare them with those files before copying any in.
