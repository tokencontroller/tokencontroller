---
title: "Overview"
description: "What Token Controller is, how token entries, rules, bills and the monthly close fit together, and how to get a first token entry placed in ten minutes."
prev:
  link: "/attribute/rules"
  label: "Writing rules that place most of your spend"
next:
  link: "/account/close"
  label: "Your first monthly close"
---

Token Controller is cost accounting and management control for AI agents. Agents report token entries, rules tag them with their ticket, project and team, and each period closes against the bill your provider actually sent. Token budgets on any tag filter close the loop: plan, record, reconcile, learn. Every part is optional. This page explains the pieces and gets a first token entry placed.

<div class="cards">
  <a class="card" href="#managers"><i></i><b>I manage people</b><span>Create the organization, connect the bill, invite people, close the first period.</span></a>
  <a class="card b" href="#engineers"><i></i><b>I manage agents</b><span>Install the Claude Code mod or the CLI, submit a first token sheet, forget it exists.</span></a>
  <a class="card s" href="/self-hosting"><i></i><b>I host it myself</b><span>One Docker image, one Postgres. The same features as the hosted Team plan.</span></a>
</div>

## How the pieces fit {#pieces}

Three things flow into the ledger, and they share the same identifiers so they can be matched against each other.

<div class="diagram" aria-label="Agents send token entries and telemetry; bills come from the provider; rules place token entries on tickets; the ledger reconciles and produces the cost sheet and reports"><svg viewBox="0 0 760 250" role="img"><defs><marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="#C6C1B4"></path></marker></defs><g font-family="Inter, sans-serif" font-size="13" fill="#141613"><rect x="10" y="20" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="43" text-anchor="middle" font-weight="600">Agents</text><text x="95" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">Claude Code, Codex, any</text> <rect x="10" y="98" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="121" text-anchor="middle" font-weight="600">Mod and CLI</text><text x="95" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">branch, PR, summary, review</text> <rect x="10" y="176" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="199" text-anchor="middle" font-weight="600">Provider</text><text x="95" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">Anthropic, OpenAI, CSV</text> <rect x="260" y="20" width="150" height="54" rx="10" fill="#EFEBE3" stroke="#DDD9CF"></rect><text x="335" y="43" text-anchor="middle" font-weight="600">Telemetry</text><text x="335" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">cost per model call</text> <rect x="260" y="98" width="150" height="54" rx="10" fill="#E2ECE5" stroke="#DDD9CF"></rect><text x="335" y="121" text-anchor="middle" font-weight="600">Token entries</text><text x="335" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">placed by rules</text> <rect x="260" y="176" width="150" height="54" rx="10" fill="#F4E7CF" stroke="#DDD9CF"></rect><text x="335" y="199" text-anchor="middle" font-weight="600">Bills</text><text x="335" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">the total to add up to</text> <rect x="490" y="72" width="150" height="106" rx="12" fill="#fff" stroke="#1C2030" stroke-width="1.5"></rect><text x="565" y="112" text-anchor="middle" font-weight="600">Ledger</text><text x="565" y="131" text-anchor="middle" fill="#6B6E67" font-size="12">per organization</text><text x="565" y="148" text-anchor="middle" fill="#6B6E67" font-size="12">per period, closed</text> <rect x="670" y="72" width="80" height="106" rx="8" fill="#1C2030"></rect><text x="710" y="119" text-anchor="middle" fill="#fff" font-weight="600" font-size="12">Cost sheet</text><text x="710" y="137" text-anchor="middle" fill="#C6C1B4" font-size="11">and reports</text></g><g stroke="#C6C1B4" stroke-width="1.4" fill="none" marker-end="url(#ar)"><path d="M180,47 L258,47"></path><path d="M180,125 L258,125"></path><path d="M180,203 L258,203"></path><path d="M410,47 C450,47 450,100 488,100"></path><path d="M410,125 L488,125"></path><path d="M410,203 C450,203 450,150 488,150"></path><path d="M640,125 L668,125"></path></g></svg></div>

| Input | What it carries | Where it comes from |
| --- | --- | --- |
| Telemetry | One telemetry event per model call: cost, tokens, model, session, request and message IDs, person, organization, repository. No text. | The agent's own OpenTelemetry exporter, pointed at your organization's ingest URL. Nothing installed on laptops. |
| Token entries | One agent session: tokens, cost, model, time range, a one-line summary, and tags such as the ticket, the project and the team. | Automatic from cloud, CI and support agents, through the GitHub Action or the token entries endpoint. From a person's machine through the Claude Code mod or the CLI, handed in on a token sheet. |
| Bills | The provider's figure for one period: gross, discount, credit and net. | Anthropic and OpenAI admin APIs, a CSV upload, manual entry, or the bills endpoint. |

<div class="note"><b>Join keys.</b> Telemetry and token entries share the session and message IDs. Token entries and bills share the period: the token entries of a period must add up to its bill. A token entry never needs telemetry to exist, and telemetry never needs a token entry to be counted. When both exist they are joined, never counted twice.</div>

## Quick start for people managers {#managers}

About fifteen minutes, once. You need an admin login at Anthropic or OpenAI to connect the bill; everything else is inside Token Controller.

<ol class="steps">
  <li><b>Create the organization.</b><p>Sign up at <a href="https://app.tokencontroller.com/signup"><code>tokencontroller.com/signup</code></a>. Pick your cost source, the way your provider charges you: an Enterprise plan at API rates, Team plan seats, Console, a cloud marketplace, or none yet. This decides what the close reconciles against, and the cost sheet will say so in plain words.</p></li>
  <li><b>Connect the bill.</b><p>Under <em>Bills</em>, add an Anthropic Enterprise Analytics key or a Console admin key, or an OpenAI admin key. The first pull loads the current and previous period. No provider API? Upload the bill as CSV once per period.</p></li>
  <li><b>Choose your tags.</b><p>Under <em>Tags</em>, list the tag names you want to report by, such as <code>team</code> and <code>client</code>. The defaults work as they are: the person answers for cost, and cost lands on a ticket, else its project, else the organization. Change the <a href="/account/tags#map">tag map</a> if teams answer for money in your organization, or if you want client as its own level.</p></li>
  <li><b>Create your projects.</b><p>Tag each project with its client, give it its ticket key pattern, for example <code>^KD-\d+</code>, and map the repositories that belong to it. Connect Jira Cloud, Azure DevOps or GitHub Issues so tickets sync every fifteen minutes. If you want the loop, give people, tickets or the project a token budget now, or accept a token forecast once there are past periods to learn from.</p></li>
  <li><b>Invite people.</b><p>Members get a link. Give each their team tag, and their token entries carry it. When they install the Claude Code mod or the CLI, their sessions turn into draft token entries and the rules you just wrote place them. For everyone on Claude Code at once, paste the <a href="#telemetry">telemetry block</a> into managed settings and skip the laptops entirely. Optionally set a cadence, such as weekly; an agent manager whose token sheet is late gets a reminder.</p></li>
  <li><b>Close the first period.</b><p>Once the period's bill is final, a controller opens <em>Close</em>. Some bills take weeks: Anthropic Enterprise figures are final <a href="/integrations/anthropic-enterprise#reconciles">about 30 days</a> after the period ends. Check the organization overhead line for cost that belongs on a project, reject anything wrong back to its agent manager with a reason, then close the period. With approval on, every token entry in the period has to be approved first. Export the cost sheet as CSV or hand finance the link.</p></li>
</ol>

## Quick start for agent managers {#engineers}

Two commands. Token entries are drafted as you work and placed by your organization's rules. You review them and submit your token sheet when you say so. If a cadence is set, submit once per cadence period.

### Install the CLI {#cli}

```text
# macOS and Linux
brew install tokencontroller/tap/tc

# Windows
winget install TokenController.tc

# connect to your organization with the invite link, or run local-only
tc connect https://tokencontroller.com/i/kestrel-4f2a
tc status
organization  Kestrel Digital       person  m.ortner@kestrel.example
agents        claude-code, codex    rules   5 organization, 2 personal
today         $212.40 spent         91% on tickets
```

### Add the Claude Code mod {#plugin}

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-controller
```

The mod runs in the terminal and in the desktop Code tab. After every turn it records Claude Code's own usage, writes the one-line summary as the session goes, and places the draft token entry. A band above the prompt shows `$212 spent · 91% on tickets · KD-214`. More on the [Claude Code mod](/collect/claude-code) page.

### Review and submit {#review}

```text
tc review
  #  when    cost     tags                              by
  1  09:12   $41.87   ticket KD-214 · team web          rule: ticket key in branch
  2  11:46   $12.10   ticket KD-214 · team web          rule: ticket key in branch
  3  13:05   $88.30   ticket KD-231 · team web          rule: ticket key in PR title
  4  15:20    $9.75   project storefront · team web     rule: repository
  5  16:41   $60.38   team web                          no ticket, 2 candidates

tc tag 5 ticket=KD-231
tc submit
submitted token sheet · 5 token entries · $212.40 · 95% on tickets
```

### Telemetry for everyone, no install {#telemetry}

Claude Code, Codex, Gemini CLI and Copilot CLI can push usage straight to your organization. For Claude Code, an administrator adds this to managed settings. It sends numbers and identifiers only; the ingest endpoint drops any event that carries prompt text.

```json
{
  "env": {
    "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
    "OTEL_METRICS_EXPORTER": "otlp",
    "OTEL_LOGS_EXPORTER": "otlp",
    "OTEL_EXPORTER_OTLP_PROTOCOL": "http/protobuf",
    "OTEL_EXPORTER_OTLP_ENDPOINT": "https://ingest.tokencontroller.com",
    "OTEL_EXPORTER_OTLP_HEADERS": "Authorization=Bearer tc_org_…",
    "OTEL_METRICS_INCLUDE_REPOSITORY": "true"
  }
}
```

<div class="note"><b>What telemetry cannot see.</b> The branch, the pull request and the one-line summary. Those come from the mod or the CLI. Organizations that only push telemetry get cost placed on projects, by repository; those that add the mod get tickets.</div>

## Concepts in one table {#concepts}

| Term | Meaning |
| --- | --- |
| Organization | One customer of Token Controller: its people, projects, tags and periods. One account belongs to one organization. It sets a reporting currency, its tags and whether approval is on. |
| Project | A body of work made of tickets, with a ticket key pattern and mapped repositories. A client is a tag on projects, such as `client: acme`; projects with the same client can be totalled. |
| Ticket | Any unit of work: a feature, a story, an epic or a bug. Synced from Jira Cloud, Azure DevOps or GitHub Issues, or created by hand. Tickets can sit inside other tickets and can carry a token budget. |
| Person | A human in the organization. The smallest cost center. |
| Tag | A name and a value on a token entry, such as `team: web` or `client: acme`. Token entries inherit tags from their person, project and ticket. Reports, token budgets and people managers' access are tag filters. |
| Role | What someone can see; each role includes the one before it. An agent manager submits token sheets and sees their own cost. A people manager reads reports, sets token budgets and cadence, and approves or rejects token entries, for the teams or projects they are assigned. A controller sees everything, runs the close and posts adjustments. |
| Agent | Software that calls a model on a person's behalf and so costs money: a coding agent on a laptop, a CI automation such as a review bot, a support agent. Token Controller keeps no record of agents, only of the cost they cause. Each token entry names the agent harness, the model and an optional agent name. |
| Agent manager | Every token entry has exactly one, who answers for its cost. Your own sessions are yours; a CI automation belongs to whoever set it up. A token entry without an agent manager shows as an error until someone takes it. |
| Token entry | One agent session's account of what it spent and on what work: tokens, cost, model, time range and a one-line summary. Its status is draft, submitted, approved, rejected or locked. |
| Token sheet | One agent manager's reviewed token entries for one cadence period: how a person confirms which ticket each session was for. Automatic token entries, from cloud, CI and support agents and from telemetry, are never on a token sheet. |
| Cadence | The rhythm in which token sheets are due, set by a people manager: weekly, every two weeks, monthly or custom. A token sheet not submitted when its cadence period is over is late, and its agent manager gets a reminder. |
| Cost center | Who answers for a cost: by default the person, or a tag such as `team` if the organization maps it so. |
| Cost object | The work a cost paid for: a ticket, a project or the organization. When no ticket matches, the cost falls back to the project, then to the organization. |
| Split | A share of a token entry with its own tags, when one session worked on two tickets. Most token entries are one split. |
| Rule | An instruction that turns signals, such as a branch or a repository, into tags. Organization rules run before personal ones. |
| Token budget | Money a human has planned for any tag filter, such as a person, a ticket, a project or a team, and a period. Typed, or accepted from a token forecast. It is compared with actual cost and never stops spending. |
| Token forecast | Money Token Controller suggests for a ticket from similar past work. It becomes a token budget only when a person accepts it. |
| Bill | A provider's figure for one period: gross, discount, credit and net. It is the total the token entries must add up to. |
| Period | One calendar month, open or closed. The close locks its token entries and tags and produces the cost sheet. An adjustment is a signed correction a controller posts to a closed period. |
| Cost sheet | The locked account of one closed period for the whole organization: cost per project and ticket, adding up to the bill. With approval on, it holds approved cost only. |
| Residual | The part of a bill that no token entry explains, such as chat usage or a restated bill, and so the only cost that is not attributed. Always shown as its own line. |

How they fit together is on the [Concepts](/concepts) page. The full list is in the [glossary](/glossary).

## List value, billed value and money {#values}

Every token entry carries a list value: its cost at the provider's published prices, taken from the agent's reported cost or computed from token counts and the price table kept with the period. A billed value exists where a bill or billed telemetry says what was actually charged. Whether list value is money depends on your cost source:

| Cost source | The close reconciles against | List value is |
| --- | --- | --- |
| Enterprise plan, usage at API rates | The Enterprise Analytics cost report at list price; the discount is its own line | Money, before discount |
| Console or API key | The Usage and Cost report, per workspace and day | Money |
| Team plan or seat-based plans | Usage credits only; usage inside the seat is not metered in dollars | A share of the seat cost, and the cost sheet says so |
| Bedrock, Vertex, Foundry | The cloud's export, uploaded as CSV | Approximate |

## Where to next {#next}
