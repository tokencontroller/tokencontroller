# One filter formula over tags and attributes is the only report and access primitive

Every figure of cost accounting is C(F, w): the sum, over every split whose tag set closed under defaults contains the filter F and whose message falls in the date range w, of fraction × message cost. F is any set of tags and attributes; w any date range. The closure applies the default tags of tag values (feature brings project, project brings client) and the attributes of the message's person with their defaults (team brings department) at read time; nothing is copied onto splits. Grouping by one tag name partitions C(F, w); filters on different names overlap and are never summed. The labels direct, project overhead and organization overhead are derived from whether the split has a feature or a project.

Management control reads cost accounting only through C(F, w): the plan P(F, w) comes from the accepted estimate spread by days, a person's plan from their shares, the deviation is P − C, and the forecast is a function of past windows. None of these writes a split.

Access is the same primitive: a grant is (person, capability, filter), and a person with view on F reads C and P for every F' that contains F. Record has no filter; adjust has the empty one. Roles are named bundles of grants, never tags or attributes.

## Why

The old model had a tag map, a ladder, a cost center, a cost object, copied inherited tags, per-report queries, token budgets with their own actual, and role scopes as a separate concept. Every one of them was a special case of "cost of a filter". One formula means one query, one set of golden cases, one place that is mutation-tested, and a report, a plan and a grant that all mean the same thing by a filter. Defaults applied at read time make a changed client or team take effect as of a date without rewriting history (ADR 0022).

## Consequences

- `crates/core` exposes one function for C(F, w) and the fold that feeds it; reports, deviations, forecasts and the reconciliation call it.
- There is no table of copied tags per split: splits hold their own tags, tag values hold their defaults, people hold their attributes with from dates.
- The authorization check for any read is "is the requested filter inside a filter the caller has view on".
