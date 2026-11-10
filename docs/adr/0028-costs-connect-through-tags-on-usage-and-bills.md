# Costs connect through tags on usage and bills

Usage and bills come in on their own, in any order, with no tags. Tags go on both afterwards, and the check compares a bill with the messages that carry the same contract tag. The docs page "How it works" draws it; the data model page holds the records and formulas. This record says how it is built.

**Sources.** Anything that brings records in is a source. Usage sources bring messages: a collector (tc, the Claude Code mod, the GitHub Action, a customer's own script) gathers usage and sends it to a usage endpoint (the token entries API, the OpenTelemetry endpoint, CSV upload). Tag sources bring tag values (Jira, a CRM, any list). Bill sources bring bills (a provider's billing API, CSV upload). OpenTelemetry is one usage endpoint that writes the same records as the API; there is no plugin system and no configurable mapping until a second format exists.

**Collection is holy.** Every usage endpoint stores what it received, with the endpoint and the time, before it does anything else. These received records live in a store of their own, outside the event log (ADR 0022), with a retention period and a real delete. Messages and activities are made from them and point back to them. When we learn to read a new kind of record, the stored ones are read again, back in time; each re-read is logged with the reader's version, and the messages it makes do not count against the daily cap (ADR 0026). The cap also counts received bytes per day.

**Privacy by default.** Content text (prompts, responses, tool output, file contents) is removed on arrival, before anything is stored, unless the organization switches keeping it on. Nothing we ship sends it unless switched on. Kept text is shown on no screen for now; it is there for later estimation and evaluation work.

**Records have open kinds.** A received record with token counts makes a message. Every other one makes an activity with a kind name, its session, its time and its signals: tool runs and counters such as commits or pull requests today, something else tomorrow. A new kind needs no change to the model. No formula reads activities yet.

**A message is a measurement.** Its own fields are person, agent, model, time and tokens by kind, plus the provider when the sender knows it. Everything else that came with it is a signal, an open list of names and values, under the key `signals` in the message format. A signal named like a tag, such as `project`, is the sender's own assignment: it runs first, wins over the organization's rules and creates the tag value when it is new. Splits sent with a session stay as they are.

**Model and provider are two things.** The provider is never guessed from the model name. A collector passes on the provider the agent reports, a sender who knows it sends it, and the contract's provider wins.

**Token kinds are open names.** Only kinds with a price row are summed. An unknown kind is listed and leaves the rest of the message priced. `cache_write` means "duration unknown" and is priced as 5 minutes; where a source gives `cache_write_5m` and `cache_write_1h`, those win for the same call.

**The contract is a built-in tag.** A contract is one thing a provider charges under, metered or flat, and holds its provider, its kind, a fee when flat and, when it has them, prices of its own. A bill has exactly one contract tag; connecting or uploading a bill creates its contract. A message gets one contract, set on the whole message before it is priced: by hand, by a rule, by a project default when all its parts agree, or because its provider has only one contract (the fallback). The fallback never chooses between two contracts of one provider. Rules resolve per tag name, so setting a project by hand does not stop the contract rule. We ship rules on what agents report about their sign-in; a sign-in rule creates its contract the first time it matches.

**Pricing.** The cost of a message is the sum, over its kinds that have a price, of tokens times the price of the contract's own row if there is one, else ours (ADR 0024). A message with no provider is priced when every provider in the price table charges the same for its model. Otherwise it is "not priced yet": its own line on every report with its token counts, never a silent zero.

**The check.** For a metered contract, bill = cost of the messages with that contract in the bill's range + unexplained, unexplained never negative. For a flat contract, bill = fee + extra usage, and the cost of its messages minus extra usage is "covered by the plan". Every check also shows the messages of the same provider with no contract and those whose contract came from the fallback, so a check cannot be made to balance by leaving messages out. Reports always show cost at the price table; cost under a flat contract is marked "on a plan", with a subtotal per kind of contract.

**Time.** Tagging runs back in time. Contract assignment may run back before the cutoff, since figures as of the cutoff stay as they were.

**Matching across sources.** When the ids of two usage sources for one session do not match, calls are matched one by one on time, model and token counts, inside one person's sessions that overlap in time, never on session totals. Both received records stay stored, and one counts.

**No conversion tooling.** There are no customers yet. Old data is ours and is read as it is.

## Why

The old model bound a bill to a provider and guessed the provider from the model name. That broke as soon as one provider charged under two contracts, such as an API account and a seat plan, and when one model was sold by several providers. It also threw away what it could not read on arrival, so a new field an agent started to send was lost for good. Treating the contract as a tag puts it on both tracks with the machinery that already exists (rules, defaults, hand assignment, running back in time), and the check becomes the same filter formula as every report (ADR 0023). Storing what arrived before reading it makes every later improvement apply to the past. Removing content text by default keeps the promise that it is not collected unless an organization asks for it.

## Consequences

- This record replaces ADR 0021. ADRs 0006, 0018 and 0024 are read with it.
- The model world test holds three contracts, four bills, two sessions started in the same minute by one person and every hard case above, with exact figures. A generated test makes up small organizations and checks that cost by any tag adds up to the total, that contracts plus "no contract" add up to the total, and that the result does not depend on the order in which messages, bills and tags arrived.
- The words door, translator and ticket system are not used.
