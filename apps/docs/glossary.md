# Token Controller

Cost accounting and management control for AI agents: token entries from agents are matched to provider bills, placed on the work that caused them, and compared with token budgets.

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
Checking, per period and per bill, that postings plus unattributed plus residual equal the bill. The cost sheet shows one line per bill and the organization total. In the loop, the step "Reconcile" is wider: it covers rules placing token entries, review, this check and the close.
_Avoid_: Matching

**Residual**:
The part of a bill that no token entry explains: usage in the provider's own chat app, per-call fees the agent harness does not report, a restated bill. Always shown as its own line.
_Avoid_: Leftover, difference

### Where cost sits

**Posting**:
The placement of a token entry, or a share of it, on one cost object with one cost kind.
_Avoid_: Allocation, assignment

**Cost center**:
The person who answers for a cost. Every posting lands on exactly one person, its agent manager. Groups and the organization total their people.
_Avoid_: Who, person side, team tree

**Cost object**:
The work a cost paid for: a ticket, a project or the organization. Ticket cost rolls up to its parent ticket and its project.
_Avoid_: What, work side

**Cost kind**:
The label on a posting that says how it relates to work: direct, project overhead, organization overhead, personal or unattributed.
_Avoid_: Category, cost type

**Direct**:
Cost placed on a ticket.

**Project overhead**:
Project cost that belongs to no single ticket, such as research or setup.

**Organization overhead**:
Cost that belongs to no project, such as internal tooling.

**Personal**:
A person's own learning and experiments.

**Unattributed**:
Spend that no rule or person has placed yet. It sits on the organization as its own line until someone places it. It is not overhead: overhead is a choice, unattributed is the absence of one.
_Avoid_: Unassigned, unallocated, other

**Attribution**:
The share of spend a rule or a person has placed, with any cost kind, as a percentage of the total. Personal and overhead count: they are accounted for. Only unattributed does not. An organization decides how specific a placement must be to count: on a project, or on a ticket.
_Avoid_: Coverage, allocation rate

**Attribution target**:
The attribution an organization aims for.

### Work

**Organization**:
One customer of Token Controller: its people, groups, projects and periods. It has one reporting currency.
_Avoid_: Account, tenant, workspace, company, client

**Project**:
A body of work made of tickets.

**Client**:
A label on a project that names who the work is for. Projects with the same client can be totalled.
_Avoid_: Customer, account

**Ticket**:
Any unit of work: a feature, a story, an epic or a bug. It has a key, and optionally a title, a description, a parent ticket and an assignee. Tickets can sit inside other tickets.
_Avoid_: Issue, task, story, work item

### People

**Person**:
A human in the organization. The smallest cost center.
_Avoid_: User, developer

**Group**:
A label on people, such as a team, a department or a discipline. A person can carry several. Groups total their people in reports and carry no token budget. Token Controller does not model an org chart.
_Avoid_: Team, department, team tree, org chart, hierarchy

**Member**:
Anyone in the organization, whatever their role.
_Avoid_: User, seat

**Agent manager**:
The role that manages agents: submits token sheets and sees its own cost. Every token entry has exactly one agent manager, who answers for its cost.
_Avoid_: Member, supervisor, owner, orchestrator, prompter, developer

**People manager**:
The role that manages people and projects: reads reports, sets token budgets and cadence, and approves or rejects token entries, for the groups or projects it is assigned. A people manager is also an agent manager.
_Avoid_: Manager

**Controller**:
The role that sees everything, runs the close and posts adjustments.
_Avoid_: Owner, admin

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
What an agent writes about what a session did, from one line to a short report. It is the only text in a token entry. On a token sheet, the agent manager sees it before submitting.

**Telemetry event**:
One model call as reported live by the agent: tokens, cost and IDs, no text.

**Door**:
A way token entries or telemetry reach Token Controller: session files, telemetry push, the GitHub Action or the token entries endpoint.
_Avoid_: Integration, connector

### Placing token entries

**Signal**:
A fact about a session that hints at its cost object: branch, working directory, repository, pull request, or a ticket key in a prompt or a title.

**Rule**:
An instruction that turns signals into postings. Organization rules run before personal ones.
_Avoid_: Filter, mapping

**Classifier**:
An optional program on a person's machine that picks among candidate tickets when rules cannot.

**Summary match**:
Token Controller placing a token entry on the open ticket its summary clearly fits, when no rule found a ticket. Always marked as such.
_Avoid_: Auto-match, AI match

**Report**:
A view people managers read, such as token budget versus actual per project.
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
Locking a period so its token entries and postings can no longer change. It produces the cost sheet. A token sheet submitted after its period closed is posted as an adjustment to that period, marked late.
_Avoid_: Month-end, lock

**Adjustment**:
A signed correction a controller posts to a closed period.

### Management control

**Token budget**:
Money a human has planned for a person, a ticket, a project or a period. It is compared with actual cost and never stops spending.
_Avoid_: Budget, limit, cap, allowance

**Token forecast**:
Money Token Controller suggests for a ticket from similar past work. It compares the ticket's title and description with the summaries of past token entries, shows the tickets they belong to with a confidence, and proposes their median cost. It becomes a token budget only when a person accepts it.
_Avoid_: Forecast, estimate, prediction

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
