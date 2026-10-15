# Token Controller

Cost accounting and management control for AI agents: token entries from agents are matched to provider bills, tagged with the work and the people that caused them, and compared with token budgets.

## Language

### The two sides

**Token entry**:
One agent session's account of what it spent and on what work: tokens, cost, model, start and end time, and a summary. It is to a token sheet what a time entry is to a timesheet.
_Avoid_: Receipt, report item, line item, usage record, voucher

**Bill**:
A provider's figure for one period's metered usage: gross, discount, credit and net, in the currency the provider charges. Metered usage is everything priced per use: tokens, and per-call fees such as web search. It is the total the token entries must add up to. Seats, subscriptions and other flat fees are not usage and stay off it. A bill exists only for metered cost sources; on a seat plan there is none.
_Avoid_: Statement, invoice

**Token sheet**:
One agent manager's reviewed token entries for one cadence period. It is how a person confirms which ticket each of their sessions was for and what it did. Telemetry can carry the money of the same sessions on its own, but never the ticket or the summary; the two are joined by session ID and counted once. Automatic token entries are never on a token sheet.
_Avoid_: Report, batch

**Automatic**:
Said of a token entry that arrives with no person involved: from cloud agents, CI agents, support agents, or built from telemetry events when no token sheet claims the session. Automatic token entries start at submitted and are never on a token sheet.
_Avoid_: Live

**Cost sheet**:
The locked account of one closed period for the whole organization: cost per project and ticket, adding up to the bill. With approval on, it holds approved cost only.
_Avoid_: Statement, close sheet, monthly report

**Pass-through**:
An organization charging a client for the cost on its cost sheet. Token Controller ends at the cost sheet; what the client is invoiced is outside it.
_Avoid_: Chargeback, rebilling, invoicing

**Provider**:
A company that charges for model usage, such as Anthropic, OpenAI or a cloud marketplace.
_Avoid_: Vendor

**Cost source**:
One way a provider charges an organization: subscription, seat, usage at API rates, API key, cloud marketplace or reseller.

**Reconciliation**:
Checking, per period and per bill, that token entries plus residual equal the bill. The cost sheet shows one line per bill and the organization total. In the loop, the step "Reconcile" is wider: it covers rules placing token entries, review, this check and the close.
Token Controller accepts everything it receives, in any order: token entries for sessions it already has, tags not yet on the list, entries for closed periods. Fitting these loose pieces together is reconciliation's job, never a reason to refuse them.
_Avoid_: Matching

**Duplicate**:
A token entry sent again for a session Token Controller already has. It is accepted and the session counts once. The copy stays visible, greyed out behind a toggle.
_Avoid_: Rejected entry

**Residual**:
The part of a bill that no token entry explains, and so the only cost that is not attributed: usage in the provider's own chat app, per-call fees the agent harness does not report, a restated bill. Always shown as its own line.
_Avoid_: Leftover, difference

### Tags

**Tag**:
A name and a value on a token entry, such as `team: growth` or `client: acme`. A token entry has at most one value per tag name, so totals by one tag name always add up. Reports, token budgets and people managers' access are all tag filters. Every tag is optional: a token entry without any still has its person.
_Avoid_: Label, dimension, attribute

**Tag name**:
The left half of a tag, such as `team`. The org manager keeps the list of tag names; anyone may add new values.
_Avoid_: Key

**Built-in tag**:
A tag name Token Controller knows and keeps records behind: `person`, `ticket` and `project`. Ticket and project values are real tickets and projects, synced from a ticket system or created by hand. Every other tag name is the organization's own.

**Inherited tag**:
A tag copied onto a token entry from the person, project or ticket it belongs to when it arrives. A person tagged `team: growth` makes their new token entries `team: growth`; a project tagged `client: acme` does the same for its token entries. Later changes reach only new token entries.

**Filter**:
A set of tags a view is limited to, such as `client: acme` or `team: web` and `project: acme-shop`. A token entry matches when it has every tag in the filter.

**Tag map**:
The organization's setting that says which tags fill the cost center and the cost object. By default the cost center is `person` and the cost object ladder is ticket, project, organization.
_Avoid_: Mapping, schema

**Ladder**:
The ordered list of tag names that fill the cost object, most specific first. A token entry's cost object is the first rung with a value; with none, it is the organization. An organization can rename, add or remove rungs, such as ticket, epic, project, client.
_Avoid_: Hierarchy, tree

### Where cost sits

**Split**:
A share of a token entry with its own tags. Most token entries are one split. A session that worked on two tickets can be split, for example 60 percent and 40 percent; the shares are a person's or a rule's judgment.
_Avoid_: Posting, allocation, assignment

**Cost center**:
Who answers for a cost: the value of the tag the tag map names, by default the person. Every split has exactly one. An organization that answers for money by team maps the cost center to `team`; each token entry still has one agent manager.
_Avoid_: Who, person side, team tree

**Cost object**:
What a cost paid for: the first rung of the ladder with a value, such as a ticket or a project, or else the organization. Ticket cost rolls up to its parent ticket and its project.
_Avoid_: What, work side

**Cost kind**:
What a split's cost object says about it: direct, project overhead or organization overhead. It is never set by hand; it follows from the ticket and project tags.
_Avoid_: Category, cost type

**Direct**:
Cost on a ticket.

**Project overhead**:
Cost on a project but on no ticket, such as research or setup.

**Organization overhead**:
Cost on no project, such as internal tooling, learning, or a session nobody tagged. It still has a person.
_Avoid_: Unattributed, unassigned, other

**Attribution**:
The share of a bill that token entries explain. Every token entry has a person, so every token entry is attributed; the rest of the bill is residual. How specific the attribution is, on a ticket, a project or only a person, is the cost kind.
_Avoid_: Coverage, allocation rate

### Work

**Organization**:
One customer of Token Controller: its people, projects, tags and periods. It has one reporting currency.
_Avoid_: Account, tenant, workspace, company, client

**Project**:
A body of work made of tickets.

**Client**:
A common tag on projects that names who the work is for, such as `client: acme`. Its projects' token entries inherit it, so cost per client is a filter. An organization can make it a rung of the ladder.
_Avoid_: Customer, account

**Ticket**:
Any unit of work: a feature, a story, an epic or a bug. It has a key, and optionally a title, a description, a parent ticket and an assignee. Tickets can sit inside other tickets.
_Avoid_: Issue, task, story, work item

### People

**Person**:
A human in the organization. The default cost center. A person can carry tags, such as `team: web` or `department: design`, which their token entries inherit. Token Controller does not model an org chart.
_Avoid_: User, developer, group, team tree

**Member**:
Anyone in the organization, whatever their role.
_Avoid_: User, seat

**Agent manager**:
The role that manages agents: submits token sheets and sees its own cost. Every token entry has exactly one agent manager, who answers for its cost.
_Avoid_: Member, supervisor, owner, orchestrator, prompter, developer

**People manager**:
The role that manages people and projects: reads reports, sets token budgets and cadence, and approves or rejects token entries, for the filters it is assigned, such as `team: web` or `client: acme`. A people manager is also an agent manager.
_Avoid_: Manager

**Org manager**:
The role that sees everything, sets up the organization, runs the close and posts adjustments. With agent managers and people managers, one of three kinds of manager.
_Avoid_: Controller, organization manager, owner, admin

**Assignee**:
The person a ticket is assigned to.
_Avoid_: Ticket owner

### Agents

**Agent**:
Software that calls a model on a person's behalf and so costs money. Token Controller keeps no record of agents, only of the cost they cause: every token entry comes from exactly one agent session. A person typing in a provider's own chat app is not an agent; that usage is residual.
_Avoid_: Bot, tool

**Agent harness**:
The product an agent ran on, such as Claude Code or Codex, carried on every token entry.

**Agent name**:
An optional label on a token entry that says which agent produced it, such as "review bot". Token entries with the same agent name can be totalled.

**Model**:
The model that did the work in a token entry. Together with the provider it decides the price.

**Subagent**:
An agent started by another agent inside the same session.

**Session**:
One continuous run of an agent, from start to end.
_Avoid_: Conversation, chat

**Segment**:
A stretch of a session on one branch and one working directory.

**Transcript**:
The full text of a session. It never leaves the machine.

**Summary**:
Free text an agent writes about what a session did, like a pull request description. It has no fixed parts; the System 1 classifier reads it as it is and matches it with estimates and tags. With the tag values, it is the only text in a token entry. On a token sheet, the agent manager sees it before submitting.

**Telemetry event**:
One model call as reported live by the agent: tokens, cost and IDs, no text.

**Door**:
A way token entries or telemetry reach Token Controller: session files, telemetry push, the GitHub Action or the token entries endpoint.
_Avoid_: Integration, connector

### Placing token entries

**Signal**:
A fact about a session that hints at its tags: branch, working directory, repository, pull request, or a ticket key in a prompt or a title.

**Rule**:
An instruction that turns signals into tags. Organization rules run before personal ones.
_Avoid_: Filter, mapping

**System 1 classifier**:
A model that reads text in one pass and answers only with probabilities: which of the given choices fits, a score on a scale, or yes or no. It never writes text, so nothing it was trained on can come back out. Token Controller's classifier is trained on the text of all organizations: summaries, estimate descriptions and the tags they got. It makes token estimates, finds similar past work, places token entries on tickets and estimates, and explains each delta.
_Avoid_: LLM, AI model, embeddings

**Classifier**:
An optional program on a person's machine that picks among candidate tickets when rules cannot.
_Avoid_: System 1 classifier

**Summary match**:
Token Controller's System 1 classifier placing a token entry on the estimate and tags its summary fits, when no rule set them. Always marked as such.
_Avoid_: Auto-match, AI match

**Report**:
A saved filter with a grouping that people managers read, such as cost by team for `client: acme`, or token budget versus actual per project.
_Avoid_: Dashboard

**Review**:
An agent manager checking their own draft token entries before submitting the token sheet.

**Submit**:
Handing in a token sheet for its cadence period.
_Avoid_: Push, sync, upload

**Late**:
Said of a token sheet whose cadence period is over and that has not been submitted.

**Approval**:
A people manager accepting submitted token entries, one by one or in bulk. An organization can switch approval off. With approval on, a period cannot be closed while a token entry in it is unapproved.
_Avoid_: Sign-off, posted

**Status**:
Where a token entry stands: draft (still on the machine), submitted (counts in reports), approved, rejected (does not count until fixed) or locked (its period is closed). Automatic token entries start at submitted.

**Reject**:
A people manager sending a token entry back to its agent manager with a reason.
_Avoid_: Decline, return

**Cadence**:
The rhythm in which token sheets are due, set by a people manager: weekly, every two weeks, monthly or custom.
_Avoid_: Reporting cadence, schedule

### Money

**Reported cost**:
The cost figure an agent supplies with its own token entry. It is optional.

**List value**:
A token entry's cost at the provider's published prices: the reported cost where there is one, otherwise token counts times the price table for the model.
_Avoid_: Estimated cost

**Billed value**:
A token entry's cost as the provider actually charged it, where a bill or billed telemetry says so.
_Avoid_: Actual cost

**Price table**:
The published prices per model used to compute list value, in USD. Each period keeps the table it was closed with.

**Unknown model**:
A model missing from the price table. Its cost is labelled, never shown as zero.

**Reporting currency**:
The one currency an organization computes in. A bill in another currency keeps its original amount and the exchange rate of its date. Any report can be shown in another currency at the rate of the period's last day.
_Avoid_: Base currency, master currency

**Share view**:
A report shown as each line's share of the period instead of money. It is the view for organizations on a seat plan, where usage is not charged in money.
_Avoid_: Percentage mode

### Time

**Period**:
One calendar month, open or closed.
_Avoid_: Billing cycle, month

**Cadence period**:
One stretch of the cadence, such as one week. Each has one token sheet per agent manager.

**Close**:
Locking a period so its token entries, splits and tags can no longer change. It produces the cost sheet. Each closed period keeps the tag map and the price table it closed with. A token sheet submitted after its period closed is posted as an adjustment to that period, marked late.
_Avoid_: Month-end, lock

**Adjustment**:
A signed correction an org manager posts to a closed period.

### Management control

**Token budget**:
Money a human has planned for a filter (a person, a ticket, a project, a team, any set of tags) and a period. It is compared with actual cost and never stops spending. Token budgets can overlap: one for `team: web` and one for `project: acme-shop` may count the same money.
_Avoid_: Budget, limit, cap, allowance

**Token estimate**:
What a described piece of work, such as a ticket or a client project, will cost, given before the work starts: a probability distribution in dollars, the chance of each cost band, with its median. The System 1 classifier makes it from the description, and from optional context such as the summaries and cost of earlier work. A token estimate is its own object: it can stand alone, or be tagged later with any tag, such as a project, an epic or a ticket. No tag name ranks above another. Anyone can ask for one through the Estimates API, without recording anything. It becomes a token budget only when a person accepts it.
_Avoid_: Forecast, quote, prediction

**Token forecast**:
Where running cost is heading: the cost of a filter, such as the organization, a team or a person, in the coming periods, extrapolated from the trend of past periods. For example, 30,000 USD last month and 35,000 USD expected next month. A token forecast looks at periods; a token estimate looks at one piece of work.
_Avoid_: Estimate, projection, prediction

**Delta**:
The difference between a token estimate's median and the actual cost of the token entries matched to it. The classifier labels why: scope (more or less was built than described), trouble (loops, failures, rework), or the estimate itself.
_Avoid_: Variance, error

**Benchmark**:
A fixed set of described tasks that real agents run every month. Token Controller publishes how close its token estimates landed, next to a plain LLM's guess.

**Actual**:
What was spent against a token budget.

**Overrun**:
Actual above token budget.

**Underrun**:
Actual below token budget.

**Outlier**:
A cost far from the organization's own baseline for similar work.
_Avoid_: Anomaly

### Plans

**Team plan**, **Enterprise plan**:
Always written with the word "plan", because both are also plan names at providers and "team" alone reads as a group of people.
