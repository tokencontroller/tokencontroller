---
title: "Overview"
description: "What Token Controller is, how its pieces fit, and how to get started as a people manager, an agent manager or a self-hoster."
---

Token Controller is cost accounting and management control for AI agents. Agents report what each session cost, rules place that cost on its ticket, project and team, and each period closes against the bill your provider actually sent. Every part is optional: you can start by only collecting cost and matching it to the bill.

<div class="cards">
  <a class="card" href="#managers"><i></i><b>I manage people</b><span>Create the organization, connect the bill, invite people, close the first period.</span></a>
  <a class="card b" href="#engineers"><i></i><b>I manage agents</b><span>Install the Claude Code mod or the CLI, submit a first token sheet, forget it exists.</span></a>
  <a class="card s" href="#self-hosting"><i></i><b>I host it myself</b><span>One Docker image, one Postgres. The same features as the hosted Team plan.</span></a>
</div>

## How the pieces fit {#pieces}

Three inputs flow into one ledger per organization and are matched against each other.

<div class="diagram" aria-label="Agents send token entries and telemetry; bills come from the provider; rules place token entries on tickets; the ledger reconciles and produces the cost sheet and reports"><svg viewBox="0 0 760 250" role="img"><defs><marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="#C6C1B4"></path></marker></defs><g font-family="Inter, sans-serif" font-size="13" fill="#141613"><rect x="10" y="20" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="43" text-anchor="middle" font-weight="600">Agents</text><text x="95" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">Claude Code, Codex, any</text> <rect x="10" y="98" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="121" text-anchor="middle" font-weight="600">Mod and CLI</text><text x="95" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">branch, PR, summary, review</text> <rect x="10" y="176" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="199" text-anchor="middle" font-weight="600">Provider</text><text x="95" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">Anthropic, OpenAI, CSV</text> <rect x="260" y="20" width="150" height="54" rx="10" fill="#EFEBE3" stroke="#DDD9CF"></rect><text x="335" y="43" text-anchor="middle" font-weight="600">Telemetry</text><text x="335" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">cost per model call</text> <rect x="260" y="98" width="150" height="54" rx="10" fill="#E2ECE5" stroke="#DDD9CF"></rect><text x="335" y="121" text-anchor="middle" font-weight="600">Token entries</text><text x="335" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">placed by rules</text> <rect x="260" y="176" width="150" height="54" rx="10" fill="#F4E7CF" stroke="#DDD9CF"></rect><text x="335" y="199" text-anchor="middle" font-weight="600">Bills</text><text x="335" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">the total to add up to</text> <rect x="490" y="72" width="150" height="106" rx="12" fill="#fff" stroke="#1C2030" stroke-width="1.5"></rect><text x="565" y="112" text-anchor="middle" font-weight="600">Ledger</text><text x="565" y="131" text-anchor="middle" fill="#6B6E67" font-size="12">per organization</text><text x="565" y="148" text-anchor="middle" fill="#6B6E67" font-size="12">per period, closed</text> <rect x="670" y="72" width="80" height="106" rx="8" fill="#1C2030"></rect><text x="710" y="119" text-anchor="middle" fill="#fff" font-weight="600" font-size="12">Cost sheet</text><text x="710" y="137" text-anchor="middle" fill="#C6C1B4" font-size="11">and reports</text></g><g stroke="#C6C1B4" stroke-width="1.4" fill="none" marker-end="url(#ar)"><path d="M180,47 L258,47"></path><path d="M180,125 L258,125"></path><path d="M180,203 L258,203"></path><path d="M410,47 C450,47 450,100 488,100"></path><path d="M410,125 L488,125"></path><path d="M410,203 C450,203 450,150 488,150"></path><path d="M640,125 L668,125"></path></g></svg></div>

| Input | What it carries | Where it comes from |
| --- | --- | --- |
| [Telemetry](/collect/telemetry) | One event per model call: cost, tokens, model, session and message IDs, person, repository. No text. | The agent's own OpenTelemetry exporter. Nothing is installed on laptops. |
| [Token entries](/collect) | One agent session: tokens, cost, model, time range, a one-line summary, and tags such as ticket, project and team. | Automatically from cloud, CI and support agents. From a person's machine through the Claude Code mod or the CLI, on a token sheet. |
| [Bills](/account/bills) | The provider's figure for one period: gross, discount, credit and net. | Anthropic and OpenAI admin APIs, CSV upload, manual entry, or the API. |

Telemetry and a token entry for the same session are joined and counted once. A period's token entries plus residual add up to its bill. [How it works](/concepts) explains the ideas behind this; every term is in the [glossary](/glossary).

## Quick start for people managers {#managers}

About fifteen minutes, once. To connect the bill you need an admin key from Anthropic or OpenAI, or the bill as CSV.

<ol class="steps">
  <li><b>Create the organization.</b><p>Sign up at <a href="https://app.tokencontroller.com/signup"><code>app.tokencontroller.com/signup</code></a> and pick your cost source: an Enterprise plan at API rates, seats, Console, a cloud marketplace, or none yet. It decides what the close reconciles against; see <a href="/account/bills#what">Bills</a>.</p></li>
  <li><b>Connect the bill.</b><p>Under <em>Bills</em>, add an Anthropic Enterprise Analytics key, a Console admin key or an OpenAI admin key. The first pull loads the current and the previous period. Without a provider API, upload the bill as CSV once per period. See <a href="/account/bills">Bills</a>.</p></li>
  <li><b>Choose your tags.</b><p>Under <em>Tags</em>, list the tag names you report by, such as <code>team</code> and <code>client</code>. The default <a href="/account/tags#map">tag map</a> works as it is; change it if teams answer for money or you want client as its own level.</p></li>
  <li><b>Create your projects.</b><p>Give each project its client tag, a ticket key pattern such as <code>^KD-\d+</code>, and its repositories, and connect Jira Cloud, Azure DevOps or GitHub Issues. See <a href="/account/projects">Projects and tickets</a>. Optionally set <a href="/account/token-budgets">token budgets</a> now.</p></li>
  <li><b>Invite people.</b><p>Give each member a <a href="/account/roles">role</a> and their team tag. They install the mod or the CLI (<a href="#engineers">below</a>), or you cover everyone on Claude Code at once with <a href="/collect/telemetry">telemetry push</a> in managed settings. Optionally set a <a href="/account/cadence">cadence</a> for token sheets.</p></li>
  <li><b>Close the first period.</b><p>When the period's bill is final, an org manager checks what is open, rejects wrong token entries back to their agent manager, and closes. Anthropic Enterprise bills are final <a href="/account/bills#anthropic-enterprise">about 30 days</a> after the period ends. Hand finance the cost sheet. See <a href="/account/close">Closing a period</a>.</p></li>
</ol>

## Quick start for agent managers {#engineers}

Your sessions become draft token entries as you work, and your organization's rules place them. You check them and submit your token sheet.

![The overview an agent manager sees: own cost this month, on tickets, to review, and the token budget](/screens/02-overview-agent-manager.png)

### Install the CLI {#cli}

```sh
# macOS and Linux
brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller
brew install tc

# Windows: tc-x86_64-pc-windows-msvc.zip from the GitHub releases

# connect with your invite link, or skip this to stay local
tc connect https://app.tokencontroller.com/i/kestrel-4f2a
```

More on the [CLI](/collect/cli) page.

### Add the Claude Code mod {#plugin}

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-org manager
```

After every turn the mod records Claude Code's usage, writes the one-line summary and places the draft token entry. A band above the prompt shows what you spent and how much is on tickets. See [Claude Code mod](/collect/claude-code).

### Review and submit {#review}

```sh
tc review                 # your draft token entries and where they were placed
tc tag 5 ticket=KD-231    # fix one that a rule got wrong
tc submit                 # submit your token sheet
```

If your organization has a cadence, submit at least once per cadence period. See [Review and submit](/attribute/review).

### Telemetry instead of an install {#telemetry}

If your administrator pushes [telemetry](/collect/telemetry), your cost is counted without any install, and it lands on projects by repository. The mod or the CLI add the branch, the pull request and the summary, so cost lands on tickets.

## Quick start for self-hosters {#self-hosting}

<ol class="steps">
  <li><b>Start Postgres and the service.</b><p>One Docker image and one Postgres database. See <a href="/self-hosting">Self-hosting</a>.</p></li>
  <li><b>Set your public URL and email.</b><p>See <a href="/self-hosting/configuration">Configuration</a>.</p></li>
  <li><b>Point the tools at it.</b><p><code>tc connect https://tc.example.com</code>, and telemetry at <code>https://tc.example.com/api/v1/ingest/otlp</code>. Then follow the <a href="#managers">quick start for people managers</a>.</p></li>
</ol>
