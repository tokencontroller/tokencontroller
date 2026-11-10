---
title: "Data model"
description: "The records Token Controller keeps, how usage and bills connect through tags, and the formulas every figure is computed with."
---

Usage and bills come in on their own, in any order, with no tags. Tags go on both afterwards, and the check compares a bill with the messages that carry the same contract. This page lists the records and the formulas. [How it works](/concepts) builds the same picture step by step, and the words are in the [glossary](/glossary).

## The picture {#picture}

| Sources | Records | Computed | Shown |
| --- | --- | --- | --- |
| Usage sources: collectors send to usage endpoints | Message, activity | Cost of each message | Reports by any tag |
| Tag sources: Jira, a CRM, any list, by hand | Tag value, attribute value | Full tag set of each split | |
| Bill sources: a provider's billing API, CSV upload | Bill | | The check of each bill |

## What arrives {#received}

Every usage endpoint stores what it received, with the endpoint and the time, before it does anything else. Messages and activities are made from these received records and point back to them. When Token Controller learns to read something new, the stored records are read again, back in time, and the re-read is logged.

Received records sit outside the organization's log, in a store of their own with a retention period, and they can be deleted. Content text (prompts, responses, tool output, file contents) is removed on arrival unless the organization switches keeping it on. See [Security and privacy](/security).

## Source records {#source}

Added by anyone or any source, never changed. A wrong one is reversed.

| | Record | Holds |
| --- | --- | --- |
| 1 | Message | One model call. Its own fields: person, agent, model, time, tokens by kind, and the provider when the sender knows it. Everything else that came with it is a [signal](#signals). An agent's reported cost is kept as a signal and is never the figure. See [Token entry schema](/reference/token-entry). |
| 2 | Activity | Any other record from a session: a kind name, the session, the time and its signals. Today tool runs and counters such as commits or pull requests. No formula reads activities yet. |
| 3 | Bill | Amount, currency, first and last day, and one contract tag. For a flat contract, the amount is the fee plus extra usage. See [Bill schema](/reference/bill). |
| 4 | Tag value | Tag name, key, optional title and description, source id, default tags. Built-in tag names: `project`, `feature`, `client`, `contract`. |
| 5 | Contract | A tag value of the built-in name `contract`, with its provider, metered or flat, the fee when flat, and its own price rows when it has them. Connecting or uploading a bill creates its contract. |
| 6 | Price table row | Provider, model, token kind, price per million, valid from, and optionally a contract. See [Price tables](/reference/pricing-tables). |
| 7 | Attribute value | Like a tag value, for people: team, department, location, with default attributes. |
| 8 | Estimate | Description, tag set, amount as a probability distribution in dollars (bands with a median), optional markup inside the amount, the estimator that made it. |
| 9 | Share | Person, project, fraction, from date, to date. |

### Signals {#signals}

A signal is a name and a value that came with a message and may help to assign it: the branch, the repository, the pull request title, how the agent was signed in, the agent's own cost figure, any attribute an OpenTelemetry exporter sent. The list is open; a sender can add any name. [Rules](/attribute/rules) read signals to put tags on.

A signal named like a tag, such as `project`, is the sender's own assignment. It runs before the organization's rules, wins over them, and creates the tag value when it is new.

### Token kinds {#token-kinds}

A token kind is an open name. Today: `input`, `output`, `cache_read`, `cache_write`, `cache_write_5m`, `cache_write_1h`.

| Kind | Priced as |
| --- | --- |
| `cache_write` | A cache write of unknown duration, priced as 5 minutes. Where a source also gives `cache_write_5m` and `cache_write_1h` for the same call, those win. |
| A kind with a price row | Its count times its price. |
| A kind with no price row | Listed on the message, left out of the sum. The rest of the message is still priced. |

## Assignments {#assignments}

Need a capability. Each supersedes the earlier assignment on the same target; the earlier one stays in the log.

| | Record | Holds |
| --- | --- | --- |
| 10 | Split | On a session or one message: a list of (fraction, tag set), fractions summing to 1. Default: one split of 100 percent; equal fractions when a session names several features and nobody decides. |
| 11 | Contract assignment | One contract on a whole message, set before it is priced. See [Which contract](#contract). |
| 12 | Accept | Makes one estimate the plan for its tag set. |
| 13 | Grant | (person, capability, filter). See [Grants and roles](/account/roles). |

## Other events {#events}

| Event | Meaning |
| --- | --- |
| Cutoff | One date per organization, moved by an org manager. See [Cutoff](/account/cutoff). |
| Submit | A person submits all their own messages inside a window. Approval is optional per organization. |
| Rule | A pattern over any signal that gives a tag value, with scope (organization or person) and order. Resolved per tag name. Runs on arrival and back in time. "Remember" on a hand assignment creates one. |
| Re-read | Received records read again by a newer reader, with the reader's version. |
| Reversal | Cancels any earlier event. |

## Which contract a message gets {#contract}

A message has at most one contract. It is set on the whole message, before pricing, by the first step that gives one:

<ol class="steps">
  <li><b>By hand.</b><p>Someone with assign sets it, or the sender sent a <code>contract</code> signal.</p></li>
  <li><b>By a rule.</b><p>A rule on any signal, such as how the agent was signed in. Rules resolve per tag name, so a project set by hand does not stop a contract rule. A shipped sign-in rule creates its contract the first time it matches.</p></li>
  <li><b>By a project default.</b><p>When every split of the message brings the same contract.</p></li>
  <li><b>By the fallback.</b><p>When the message's provider has exactly one contract. The fallback never chooses between two contracts of one provider.</p></li>
</ol>

A message none of these reach has no contract and shows in the [review list](/attribute/review), where one answer may cover many messages. Contract assignment may run back before the cutoff, since figures as of the cutoff stay as they were.

The provider of a message is the contract's provider when it has a contract, else the one the sender reported. It is never guessed from the model's name.

## Formulas {#formulas}

Cost accounting. _t(e)_ is the time of message _e_, _p_ the price table, _F_ a filter, _w_ a date range, _c_ a contract.

| | | |
| --- | --- | --- |
| (1) | C(e) = Σ over kinds k with a price: tokens_k(e) × p(c(e), provider(e), model(e), k, as of t(e)) | Cost of a message. The contract's own row wins over ours. No provider: priced when every provider in the table charges the same for the model, else not priced yet. No counts: a count error. Never a silent zero. |
| (2) | T*(s) = the split's tag set closed under defaults, plus the message's contract | Feature brings project, project brings client; the person brings their attributes, team brings department. |
| (3) | C(F, w) = Σ fraction(s) × C(e(s)) over splits s with F ⊆ T*(s) and t(e) ∈ w | The only formula reports use. F is any set of tags and attributes. |
| (4) | Σ over the values of one tag name + C(no value) = C(F, w) | The values of one name partition the total. For `contract` this holds per message. Filters on different names are never summed. |
| (5) | Metered: Bill(c, r) = C({contract: c}, r) + U(c, r), U ≥ 0 | The check on the bill's own range. U is unexplained, a line on the bill. U < 0 means a message is wrong. |
| (6) | Flat: Bill(c, r) = fee(c, r) + X(c, r), and covered(c, r) = C({contract: c}, r) − X(c, r) | X is extra usage. Covered by the plan is what the plan was worth, shown for full months where the fee is known. |
| | Beside every check | Messages of the same provider with no contract, and messages whose contract came from the fallback. |
| | Labels | A split with a feature is direct; with a project and no feature, project overhead; with neither, organization overhead. Cost under a flat contract is also marked "on a plan". |

Reports show cost at the price table, the standard cost in controlling terms. The bills give the actual cost. Under a flat contract the two differ by design, and the difference is "covered by the plan".

Management control. Reads cost accounting only through C(F, w).

| | | |
| --- | --- | --- |
| (7) | Ĉ(w) = f(description(w), H) | An estimate from any estimator f. H is every tag value with its cost so far, last activity and source status. |
| (8) | P(F, w) = amount of the accepted estimate for F, spread over w by the days of its date range | The plan. Markup is inside the amount. |
| (9) | P(person q, w) = Σ over shares of q: fraction × P(project, w ∩ [from, to]) | A person's plan. Fractions over people may exceed 1; shown, never corrected. |
| (10) | Δ(F, w) = P(F, w) − C(F, w) | Deviation. Positive is favourable. |
| (11) | C̃(F, next w) = g over past windows | Forecast. A report, not a record. |
| (12) | H grows with every tagged value; f is refit | Learning. |

The [worked month](/concepts#month) runs the cost formulas on three bills.

## Rules that always hold {#rules}

<ol class="steps">
  <li><b>Nothing that arrives is thrown away.</b><p>Received records are stored before they are read, except content text an organization did not switch on.</p></li>
  <li><b>Nothing in the log is edited.</b><p>A correction is a reversal or a superseding assignment. Every figure is a fold of the log as of a date.</p></li>
  <li><b>Every message has a person.</b><p>For a service token, the person who created it.</p></li>
  <li><b>One value per tag name on a split, one contract per message.</b><p>So cost grouped by one tag name adds up to the total.</p></li>
  <li><b>Only messages are summed.</b><p>Bills are checked against them, plans are compared with them.</p></li>
  <li><b>Nothing is guessed.</b><p>The provider never follows from the model's name. Unknown is shown as unknown, and a message not priced yet is its own line with its token counts.</p></li>
  <li><b>A check cannot balance by leaving messages out.</b><p>Messages of the same provider with no contract always show beside it.</p></li>
  <li><b>Management control never writes into cost accounting.</b><p>Estimates, accepts and shares read C(F, w) and change no split.</p></li>
</ol>
