# Token Controller

Cost accounting and management control for AI agents. Usage and bills come in on their own; tags go on both afterwards, so each bill can be checked against the messages under the same contract. The price table turns token counts into cost, tags and attributes say what and who the cost was for, and plans say what it was meant to be.

Each term may have an _Also known as_ line: the word a reader from controlling, an ERP system or OpenTelemetry may already know for the same idea. Those words are close, not always identical, and the docs use only the term itself.

## Language

### Sources

**Source**:
Anything that brings records in. There are three kinds: usage sources bring messages, tag sources bring tag values, bill sources bring bills. They load on their own and in any order.
_Also known as_: Feeder system (ERP), source system

**Usage source**:
What brings messages and activities: a collector sending to a usage endpoint.

**Collector**:
Software of ours or yours that gathers usage where agents run and sends it to a usage endpoint: tc, the Claude Code mod, the GitHub Action or your own script. The OpenTelemetry Collector is a separate product; you may run it in front of our OpenTelemetry endpoint, and then it is part of your collector.
_Also known as_: Exporter (OpenTelemetry), agent (monitoring tools)
_Avoid_: Door, translator

**Usage endpoint**:
Where usage arrives at Token Controller: the token entries API, the OpenTelemetry endpoint and CSV upload. Each one stores what it received before reading it.
_Also known as_: Receiver (OpenTelemetry), ingest endpoint
_Avoid_: Door, integration, connector

**Tag source**:
What brings tag values: Jira, Azure DevOps or GitHub Issues, a CRM, any list, or a person typing them.
_Also known as_: Master data (controlling, ERP)
_Avoid_: Ticket system

**Bill source**:
What brings bills: a provider's billing API, or a CSV upload.
_Also known as_: Accounts payable, vendor invoices

**Received record**:
What a usage endpoint received, as it arrived, with the endpoint and the time. Messages and activities are made from it and point back to it. It is kept in a store of its own with a retention period and can be deleted. When Token Controller learns to read something new, received records are read again, back in time.
_Also known as_: Raw event, landing zone (data warehousing)

**Content text**:
Prompts, responses, tool output and file contents. Removed on arrival unless the organization switches keeping it on. Nothing Token Controller ships sends it unless switched on.

### Records

**Source record**:
A record any source adds and nobody ever changes: a message, an activity, a bill, a tag value, an attribute value, an estimate, a share or a price table row. A wrong one is reversed, never edited.

**Assignment**:
A record that needs a capability and supersedes the earlier assignment on the same target: a split, an accept or a grant. The earlier one stays in the log.

**Event**:
An entry in an organization's one append-only log: source records, assignments, the cutoff, submits, approvals, rules, re-reads and reversals. Nothing in it is edited. Every figure is computed from the log when you look at it. An event is not an activity, and not an OpenTelemetry event.
_Also known as_: Journal entry (accounting)
_Avoid_: Row, transaction

**Reversal**:
An event that cancels an earlier event. The earlier one stays visible and stops counting. It is the only way to correct anything.
_Avoid_: Edit, delete, adjustment, Storno

**Message**:
One model call: person, agent, model, time and tokens by kind, plus the provider when the sender knows it. Everything else that came with it is a signal. It is the only thing that is ever summed.
_Also known as_: Source document, voucher (accounting); model call, request
_Avoid_: Telemetry row, entry

**Activity**:
Any record from a session that is not a model call: a kind name, the session, the time and its signals, such as a tool run or a pull request opened. Kinds are open, so a new kind needs no change. No figure uses activities yet.
_Also known as_: Event, metric (OpenTelemetry)
_Avoid_: Session event

**Session**:
One continuous run of an agent, from start to end, and the messages in it. A session carries session facts: turns, tool calls, retries and reruns, duration, subagents, files touched, a result pointer such as a pull request URL, and a summary.
_Avoid_: Conversation, chat

**Token entry**:
The API noun for one message or one session record, as the token entries endpoint and the CLI send them. A token entry is one message, or one session with its messages.

**Bill**:
What a provider charges under one contract for a date range: amount, currency, first and last day, and one contract tag. For a metered contract it is the usage. For a flat contract it is the fee plus extra usage.
_Also known as_: Vendor invoice, accounts payable item
_Avoid_: Statement

**Estimate**:
What a described piece of work will cost: a probability distribution in dollars, bands with a median, with an optional markup inside the amount. Made by any estimator for a tag set. Many estimates may exist for one tag set; the accepted one is the plan.
_Avoid_: Token estimate, quote, prediction, forecast

**Tag value**:
A record behind a tag: tag name, key, optional title and description, a source id when a tag source owns it, and its default tags. Tag values are records, never bare strings.

**Share**:
A person's fraction of a project's plan between two dates. Shares over the people of a project may add up to more than 1; that is shown, never corrected. A handover is a new share for the new person.
_Avoid_: Allocation, assignment

**Price table**:
Rows of provider, model, token kind, price per million tokens and the day the price is valid from. Ours is the default; a contract may carry rows of its own, which win. It is the only thing that turns token counts into cost.
_Also known as_: Standard cost rates (cost accounting), rate card

**Attribute value**:
Like a tag value, for people: team, department, location, each with default attributes, such as team web bringing department engineering. Attributes are a different word from tags on purpose: tags say what work was for, attributes say who the person is.

**Split**:
An assignment on a session or one message: a list of fractions and tag sets that add up to 1. Without one, a session is one split of 100 percent on the tags its rules found, or equal fractions when it names several features and nobody decides.
_Avoid_: Posting, allocation, segment

**Accept**:
The assignment that makes one estimate the plan for its tag set. Accepting another estimate supersedes it; the earlier one stays a record.

**Grant**:
(person, capability, filter): what a person may do, and on which part of the organization. See Access.
_Avoid_: Permission, right, scope

**Rule**:
A pattern over any signal that gives a tag value, with a scope (the organization or one person) and an order. Rules resolve per tag name, so a project set by hand does not stop a contract rule. A rule runs when a message arrives and runs back over messages since the cutoff; a contract rule may run back before it. "Remember" on a hand assignment creates one.
_Also known as_: Allocation rule, posting rule (accounting)
_Avoid_: Mapping

**Signal**:
A name and a value that came with a message and may help to assign it: the branch, the repository, how the agent was signed in, any attribute an exporter sent. The list is open. Rules read signals to put tags on. A signal named like a tag, such as `project`, is the sender's own assignment and wins over rules. OpenTelemetry uses the word "signal" for traces, metrics and logs; our signal is what OpenTelemetry calls an attribute.
_Also known as_: Attribute, resource attribute (OpenTelemetry), label, metadata

**Re-read**:
Reading stored received records again with a newer reader, back in time. Each re-read is logged with the reader's version.

### Tags and attributes

**Tag**:
A tag name with a tag value, such as `project: shop` or `contract: claude-team`. Tags say what the work was for. They go on messages through splits, and on bills through their contract. A split has at most one value per tag name, so cost grouped by one tag name adds up to the total.
_Also known as_: Cost object (cost accounting), dimension
_Avoid_: Label

**Tag name**:
The left half of a tag. Built in: `project`, `feature`, `client` and `contract`. An organization adds its own, such as `purpose`.
_Avoid_: Key

**Project**:
A built-in tag name for a body of work. Its default tags usually name a client.

**Feature**:
A built-in tag name for any unit of work: a ticket, story, epic, task or bug are all features. A feature's default tags name its project. Synced from a tag source such as Jira, or created by hand.
_Also known as_: Work item, ticket, issue
_Avoid_: Story, task

**Client**:
A built-in tag name for who the work is for. A project's default tags name its client, so cost per client is a filter.
_Avoid_: Customer, account

**Contract**:
A built-in tag name for one thing a provider charges you under, such as an API account or a seat plan. A contract has its provider, is metered or flat, holds the fee when flat, and may carry its own prices. A bill has exactly one contract; a message has at most one, set on the whole message before it is priced. Connecting or uploading a bill creates its contract.
_Also known as_: Vendor agreement, billing account, subscription

**Metered contract**:
A contract billed per use. Its bill should equal the cost of its messages plus what no message explains.
_Also known as_: Pay as you go, usage-based billing

**Flat contract**:
A contract with a fixed fee, such as a seat plan, that may add extra usage on top. Its bill is fee plus extra usage. Cost under it is marked "on a plan".
_Also known as_: Subscription, seat plan, fixed price

**Fallback**:
The last way a message gets a contract: its provider has exactly one contract. The fallback never chooses between two contracts of one provider, and every check shows the messages it placed.

**Default tags**:
The tags a tag value brings with it: a feature brings its project, a project brings its client. The same for attributes: a team brings its department. Defaults are applied when a figure is computed, so a filter on `client: acme` finds every message on a feature of one of Acme's projects.
_Avoid_: Inherited tags, tag map, ladder, hierarchy, rollup

**Attribute**:
An attribute name with an attribute value on a person, such as `team: web` or `department: engineering`. A message carries its person's attributes as of its time. Tags say what the work was for, attributes say who the person is. Not to be mixed up with an OpenTelemetry attribute, which arrives as a signal.
_Also known as_: Cost centre (cost accounting), org unit
_Avoid_: Person tag, org chart

**Filter**:
Any set of tags and attributes, such as `client: acme` or `team: web` and `project: shop`. The empty filter is the whole organization. Every report, every grant and every plan is a filter.
_Avoid_: Scope, segment

**Label**:
What a split's tags say about how specific it is: direct with a feature, project overhead with a project and no feature, organization overhead with neither. It still has its person. Nobody sets it.
_Avoid_: Cost kind, category, cost type

**Direct**:
Cost on a feature.

**Project overhead**:
Cost on a project but on no feature, such as research or setup.

**Organization overhead**:
Cost on no project. It still has its person.
_Avoid_: Unattributed, unassigned, other

### Money

**Cost**:
Of a message: the sum, over its token kinds that have a price, of tokens times the price table for its contract or provider, model and kind as of its time. Of a filter over a date range: the sum of fraction times message cost over every split the filter matches. The one formula every report uses. Reports always show cost at the price table.
_Also known as_: Standard cost (cost accounting)
_Avoid_: List value, billed value, actual as a record

**Token kind**:
An open name for what tokens were spent on: `input`, `output`, `cache_read`, `cache_write` and whatever comes next. Only kinds with a price row are summed; an unknown kind is listed and leaves the rest priced. `cache_write` means a cache write of unknown duration and is priced as 5 minutes; `cache_write_5m` and `cache_write_1h` win where a source gives them.
_Also known as_: Cost type (cost accounting)

**Not priced yet**:
A message the price table cannot price for certain, such as a model missing from it, or no provider and different prices among providers. Its own line on every report with its token counts, never a silent zero.
_Avoid_: Unknown model

**On a plan**:
The mark on cost under a flat contract. Reports show a subtotal per kind of contract, so cost paid per token and cost covered by a fee are never mixed up.

**Covered by the plan**:
For a flat contract over a full month with a known fee: the cost of its messages minus extra usage. What the plan was worth that month.
_Also known as_: Variance (cost accounting)

**Count error**:
A message with no token counts. It has no cost and is shown as a count error, never as zero.

**Reported cost**:
A cost figure an agent sends with its own message. It is kept as a signal and is never the figure Token Controller shows.

**Reconciliation**:
The check of one bill on its own date range against the messages with the same contract. Metered: the bill equals the cost of those messages plus what no message explains, which is never negative; negative means a message is wrong. Flat: the bill equals the fee plus extra usage. Beside every check stand the messages of the same provider with no contract and those placed by the fallback, so a check cannot balance by leaving messages out. Also called the check.
_Also known as_: The check; bank reconciliation, three-way match (accounting)
_Avoid_: Residual, matching, cost sheet, close

**Provider**:
Who charges you for model usage, such as Anthropic, OpenAI or Azure Foundry. Not the same as who made the model: OpenAI's GPT may be charged by Azure Foundry. Sent with a message when known; the contract's provider wins. Never guessed from the model's name.
_Also known as_: Vendor, supplier

**Reporting currency**:
The one currency an organization shows figures in. The price table is in US dollars per million tokens; a bill keeps its own currency and the exchange rate of its last day.
_Avoid_: Base currency

**Pass-through**:
An organization charging a client for the cost on a filter. What the client is invoiced is outside Token Controller.
_Avoid_: Chargeback, rebilling

### Plans

**Plan**:
The amount of the accepted estimate for a filter, spread over a date range by days. For a person, the sum of their shares times the plans of those projects. A plan never stops spending.
_Avoid_: Token budget, budget, limit, cap, allowance

**Estimator**:
Any function that turns a description and history into an estimate. Today it is the System 1 classifier; a person typing a figure is one too.

**System 1 classifier**:
A model that reads text in one pass and answers only with probabilities. It never writes text, so nothing it was trained on can come back out. It makes estimates from descriptions and the history of every tag value, and labels each deviation.
_Avoid_: LLM, AI model, embeddings, classifier program

**Markup**:
An overhead percentage inside an estimate's amount. There is no separate markup line.

**Deviation**:
Plan minus cost for any filter and date range, written Δ. Positive is favourable. The System 1 classifier labels why: scope, trouble, or the estimate itself.
_Avoid_: Delta, variance, overrun, underrun

**Forecast**:
Where cost is heading: the cost of a filter in the next date range, computed from past ranges. A forecast is a report, never a record.
_Avoid_: Token forecast, projection

**Benchmark**:
A fixed set of described tasks that real agents run every month. Token Controller publishes how close its estimates landed, next to a plain LLM's guess.

### Access

**Capability**:
One of seven things a grant allows: view, record, assign, approve, plan, grant, adjust.

**View**:
Read cost and plan for a filter and every filter inside it.

**Record**:
Send messages and tag them with keys you were told. Everyone has it, with no filter.

**Assign**:
Tag, split and correct other people's messages inside a filter.

**Approve**:
Approve or reject submissions inside a filter.

**Plan capability**:
Create estimates, accept plans and set shares inside a filter.

**Adjust**:
Add events dated before the cutoff. Org managers only.

**Role**:
A named bundle of grants: agent manager, people manager, org manager. A role is never a tag or an attribute.

**Agent manager**:
The default role: record, and view on `person: self`. Everyone who runs an agent.
_Avoid_: Member, developer, user

**People manager**:
Agent manager plus view, assign, approve, plan and grant on the filters they were granted, such as `team: web` or `client: acme`.
_Avoid_: Manager

**Org manager**:
Every capability on the empty filter, plus adjust and moving the cutoff.
_Avoid_: Controller, admin, owner

**Person**:
A human in the organization. Every message has one. A person carries attributes.
_Avoid_: User, seat

**Organization**:
One customer of Token Controller: one event log, one cutoff, one reporting currency.
_Avoid_: Account, tenant, workspace

### Agents

**Agent**:
Software that calls a model on a person's behalf. Token Controller keeps no record of agents, only their messages. A person typing in a provider's own chat app is not an agent; that usage is what the bill has that no message explains.
_Avoid_: Bot, tool

**Agent harness**:
The product an agent ran on, such as Claude Code or Codex, carried on every message.

**Agent name**:
An optional label on a message, such as "review bot", so one automation can be totalled across projects.

**Model**:
What ran, with its maker, such as OpenAI's GPT or Anthropic's Claude. With the contract or provider and the token kind it picks the price table row.

**Subagent**:
An agent started by another agent inside the same session. One of the session facts.

**Summary**:
Free text an agent writes about what a session did. Kept like the tag values and estimate descriptions; other content text only when the organization switches it on.

**Transcript**:
The full text of a session. It stays on the machine unless the organization switches keeping content text on.

**OpenTelemetry**:
An open standard many agents use to report what they do. Token Controller's OpenTelemetry endpoint is one usage endpoint and writes the same records as the token entries API.

**Duplicate**:
The same model call seen through two usage sources. Both received records are kept; one message counts. When the ids do not match, calls are matched one by one on time, model and token counts, inside one person's sessions that overlap in time.

**Review list**:
The messages that wait for a person's answer: no project, no contract, provider unknown. One answer may cover many messages and can become a rule.
_Also known as_: Clearing account, suspense account (accounting)
_Avoid_: Check list, inbox

### Time

**As of**:
The date up to which the event log is read. As of today gives the current figures; as of the cutoff gives the frozen ones. Every report shows its as-of date.
_Avoid_: Snapshot, version

**Cutoff**:
One date per organization. Figures as of the cutoff are frozen; only an org manager moves it or adds events dated before it. It replaces closing months.
_Avoid_: Period, close, month-end, lock, closed period

**Submit**:
A person confirming all their own messages inside a date window. Approval of a submission is optional per organization: approve, or reject with a reason.
_Avoid_: Token sheet, timesheet, cadence

**Date range**:
Any first and last day a report, a bill or a plan covers.
_Avoid_: Period, billing cycle

### Plans we sell

**Team plan**, **Enterprise plan**:
Always written with the word "plan", because both are also plan names at providers and "team" alone reads as a group of people.
