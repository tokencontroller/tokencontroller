# Token Controller

Cost accounting and management control for AI agents: every model call an agent makes is a message with its token counts, the price table turns counts into cost, tags and attributes say what and who the cost was for, and plans say what it was meant to be.

## Language

### Records

**Source record**:
A record anyone or any door adds and nobody ever changes: a message, a bill, an estimate, a tag value, an attribute value, a share or a price table row. A wrong one is reversed, never edited.

**Assignment**:
A record that needs a capability and supersedes the earlier assignment on the same target: a split, an accept or a grant. The earlier one stays in the log.

**Event**:
Every record and every action in an organization is an event in one append-only log: source records, assignments, the cutoff, submits, approvals, rules and reversals. Nothing is edited or deleted. Every figure is computed from the log when you look at it.
_Avoid_: Row, transaction

**Reversal**:
An event that cancels an earlier event. The earlier one stays visible and stops counting. It is the only way to correct anything.
_Avoid_: Edit, delete, adjustment, Storno

**Message**:
One model call inside an agent session: person, provider, model, token counts by kind (input, output, cache read, cache write), time, session id, message id, agent harness and an optional agent name. Never prompts. It is the only thing that is ever summed.
_Avoid_: Model call, request, telemetry row, entry

**Session**:
One continuous run of an agent, from start to end, and the messages in it. A session carries session facts: turns, tool calls, retries and reruns, duration, subagents, files touched, a result pointer such as a pull request URL, and a summary.
_Avoid_: Conversation, chat

**Token entry**:
The API noun for one message or one session record, as the token entries endpoint and the CLI send them. A token entry is one message, or one session with its messages.

**Bill**:
A provider's figure for metered usage over a date range: provider, first and last day, amount and currency. Metered usage is everything priced per use: tokens and per-call fees. Seats, subscriptions and other flat fees are not on it.
_Avoid_: Statement, invoice

**Estimate**:
What a described piece of work will cost: a probability distribution in dollars, bands with a median, with an optional markup inside the amount. Made by any estimator for a tag set. Many estimates may exist for one tag set; the accepted one is the plan.
_Avoid_: Token estimate, quote, prediction, forecast

**Tag value**:
A record behind a tag: tag name, key, optional title and description, a source id when a ticket system owns it, and its default tags. Tag values are records, never bare strings.

**Share**:
A person's fraction of a project's plan between two dates. Shares over the people of a project may add up to more than 1; that is shown, never corrected. A handover is a new share for the new person.
_Avoid_: Allocation, assignment

**Price table**:
Rows of provider, model, token kind, price per million tokens and the day the price is valid from. It is the only thing that turns token counts into cost. Required: without a price for a model, a message has no cost.
_Avoid_: Pricing, rate card

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
A pattern over signals that gives a tag value, with a scope (the organization or one person) and an order. A rule writes split assignments when a message arrives and runs back over unassigned messages since the cutoff. "Remember" on a hand assignment creates one.
_Avoid_: Mapping, filter

**Signal**:
A fact about a session that is not a tag: branch, repository, pull request title, keys found in prompts, values in a repository's config file, telemetry resource attributes. Rules turn signals into tag values. A signal that matches no tag value leaves the message with no project.

### Tags and attributes

**Tag**:
A tag name with a tag value on a split, such as `project: shop` or `feature: KD-214`. A split has at most one value per tag name, so cost grouped by one tag name adds up to the total.
_Avoid_: Label, dimension, cost object, cost center

**Tag name**:
The left half of a tag. Built in: `project`, `feature` and `client`. An organization adds its own, such as `purpose`.
_Avoid_: Key

**Project**:
A built-in tag name for a body of work. Its default tags usually name a client.

**Feature**:
A built-in tag name for any unit of work: a ticket, story, epic, task or bug are all features. A feature's default tags name its project. Synced from Jira or created by hand.
_Avoid_: Ticket, issue, story, task, work item

**Client**:
A built-in tag name for who the work is for. A project's default tags name its client, so cost per client is a filter.
_Avoid_: Customer, account

**Default tags**:
The tags a tag value brings with it: a feature brings its project, a project brings its client. The same for attributes: a team brings its department. Defaults are applied when a figure is computed, so a filter on `client: acme` finds every message on a feature of one of Acme's projects.
_Avoid_: Inherited tags, tag map, ladder, hierarchy, rollup

**Attribute**:
An attribute name with an attribute value on a person, such as `team: web` or `department: engineering`. A message carries its person's attributes as of its time.
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
Of a message: its token counts by kind times the price table for its provider and model as of its time. Of a filter over a date range: the sum of fraction times message cost over every split the filter matches. The one formula every report uses.
_Avoid_: List value, billed value, actual as a record

**Count error**:
A message with no token counts. It has no cost and is shown as a count error, never as zero.

**Unknown model**:
A model that is missing from the price table on the message's day. Its cost is labelled, never shown as zero.

**Reported cost**:
A cost figure an agent sends with its own message. It is kept as a fact about the message and is never the figure Token Controller shows.

**Reconciliation**:
The check on a bill's own date range: the bill equals the cost of every message of that provider in the range plus what the bill has that no message explains. The second part is a line on the bill and is never negative. Negative means a message is wrong.
_Avoid_: Residual, matching, cost sheet, close

**Provider**:
A company that charges for model usage, such as Anthropic, OpenAI or a cloud marketplace.
_Avoid_: Vendor

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
The model that answered a message. With the provider and the token kind it picks the price table row.

**Subagent**:
An agent started by another agent inside the same session. One of the session facts.

**Summary**:
Free text an agent writes about what a session did. With tag values and estimate descriptions, the only text Token Controller stores.

**Transcript**:
The full text of a session. It never leaves the machine.

**Telemetry event**:
One message as the agent reports it live over OpenTelemetry: counts, model and ids, no text.

**Door**:
A way messages reach Token Controller: session files through the CLI, the Claude Code mod, telemetry push, the GitHub Action, the token entries endpoint, manual entry.
_Avoid_: Integration, connector

**Duplicate**:
The same message seen through two doors. It is one record and counts once.

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
