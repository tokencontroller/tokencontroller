---
title: "Data model"
description: "The ten record types, the events, and the eleven formulas every figure is computed with."
---

An organization is an append-only log of events. The records below are what the log holds; the formulas are how every figure is computed from it, as of a date. The terms are in the [glossary](/glossary); [How it works](/concepts) shows them together.

## Source records {#source}

Added by anyone or any door, never changed.

| | Record | Holds |
| --- | --- | --- |
| 1 | Message | Person, provider, model, token counts by kind (input, output, cache read, cache write), time, session id, message id, agent harness, optional agent name. Session facts: turns, tool calls, retries and reruns, duration, subagents, files touched, result pointer, summary. Never prompts. An agent's reported cost may be stored as a fact; it is never the figure. See [Token entry schema](/reference/token-entry). |
| 2 | Bill | Provider, date range, amount, currency. Metered usage only. See [Bill schema](/reference/bill). |
| 3 | Estimate | Description, tag set, amount as a probability distribution in dollars (bands with a median), optional markup inside the amount, the estimator that made it. Many may exist for one tag set. |
| 4 | Tag value | Tag name, key, optional title and description, source id, default tags. Built-in tag names: `project`, `feature`, `client`. |
| 5 | Share | Person, project, fraction, from date, to date. |
| 6 | Price table row | Provider, model, token kind, price per million, valid from. See [Price tables](/reference/pricing-tables). |
| 7 | Attribute value | Like a tag value, for people: team, department, location, with default attributes. |

## Assignments {#assignments}

Need a capability. Each supersedes the earlier assignment on the same target; the earlier one stays in the log.

| | Record | Holds |
| --- | --- | --- |
| 8 | Split | On a session or one message: a list of (fraction, tag set), fractions summing to 1. Default: one split of 100 percent; equal fractions when a session names several features and nobody decides. |
| 9 | Accept | Makes one estimate the plan for its tag set. |
| 10 | Grant | (person, capability, filter). See [Grants and roles](/account/roles). |

## Other events {#events}

| Event | Meaning |
| --- | --- |
| Cutoff | One date per organization, moved by an org manager. See [Cutoff](/account/cutoff). |
| Submit | A person submits all their own messages inside a window. Approval of a submission is optional per organization: approve, or reject with a reason. |
| Rule | A pattern over signals that gives a tag value, with scope (organization or person) and order. Writes splits on arrival and runs back over unassigned messages since the cutoff. "Remember" on a hand assignment creates one. |
| Reversal | Cancels any earlier event. |

Signals are not records: branch, repository, pull request title, keys in prompts, config file values, telemetry resource attributes. They are carried with the session and read by rules.

## Formulas {#formulas}

Cost accounting. _t(e)_ is the time of message _e_, _p_ the price table, _F_ a filter, _w_ a date range.

| | | |
| --- | --- | --- |
| (1) | C(e) = Σ over kinds of tokens_kind(e) × p(provider, model, kind, as of t(e)) | Cost of a message. No counts: a count error, never zero. Unknown model: labelled, never zero. |
| (2) | T*(s) = the split's tag set closed under defaults | Feature brings project, project brings client; the person brings their attributes, team brings department. |
| (3) | C(F, w) = Σ fraction(s) × C(e(s)) over splits s with F ⊆ T*(s) and t(e) ∈ w | The only formula reports use. F is any set of tags and attributes. |
| (4) | Bill(provider, r) = C({provider}, r) + R(provider, r), R ≥ 0 | Reconciliation on the bill's own range. R is a line on the bill. R < 0 means a message is wrong. |
| (5) | Σ over the values of one tag name = C(F, w) | The values of one name partition the total. Filters on different names are never summed. |
| | Labels | A split with a feature is direct; with a project and no feature, project overhead; with neither, organization overhead. |

Management control. Reads cost accounting only through C(F, w).

| | | |
| --- | --- | --- |
| (6) | Ĉ(w) = f(description(w), H) | An estimate from any estimator f. H is every tag value with its cost so far, last activity and source status. |
| (7) | P(F, w) = amount of the accepted estimate for F, spread over w by the days of its date range | The plan. Markup is inside the amount. |
| (8) | P(person q, w) = Σ over shares of q: fraction × P(project, w ∩ [from, to]) | A person's plan. Fractions over people may exceed 1; shown, never corrected. |
| (9) | Δ(F, w) = P(F, w) − C(F, w) | Deviation. Positive is favourable. Labelled scope, trouble, or the estimate itself. |
| (10) | C̃(F, next w) = g over past windows | Forecast. A report, not a record. |
| (11) | H grows with every tagged value; f is refit | Learning. |

The [worked month](/concepts#month) runs every formula on one bill.

## Rules that always hold {#rules}

<ol class="steps">
  <li><b>Nothing is edited or deleted.</b><p>A correction is a reversal or a superseding assignment. Every figure is a fold of the log as of a date.</p></li>
  <li><b>Every message has a person.</b><p>For a service token, the person who created it.</p></li>
  <li><b>One value per tag name on a split.</b><p>So cost grouped by one tag name adds up to the total.</p></li>
  <li><b>Only messages are summed.</b><p>Bills are reconciled against them, plans are compared with them. Nothing else carries cost.</p></li>
  <li><b>Management control never writes into cost accounting.</b><p>Estimates, accepts and shares read C(F, w) and change no split.</p></li>
</ol>
