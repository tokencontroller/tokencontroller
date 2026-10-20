---
title: "Overview"
description: "What Token Controller is, how its pieces fit, and how to get started as a people manager, an agent manager or a self-hoster."
---

Token Controller is cost accounting and management control for AI agents. Agents send every model call as a message with its token counts, the price table turns counts into cost, rules put each session on its feature and project, and the provider's bill is reconciled against the sum. Every part is optional: you can start by only collecting messages and checking them against the bill.

<div class="cards">
  <a class="card" href="#managers"><i></i><b>I manage people</b><span>Create the organization, load the price table, connect the bill, invite people.</span></a>
  <a class="card b" href="#engineers"><i></i><b>I manage agents</b><span>Install the Claude Code mod or the CLI, submit once, forget it exists.</span></a>
  <a class="card s" href="#self-hosting"><i></i><b>I host it myself</b><span>One Docker image, one Postgres. The same features as the hosted Team plan.</span></a>
  <a class="card" href="#api"><i></i><b>I build on the API</b><span>Report usage from any agent, read every report, ask what work will cost.</span></a>
</div>

## How the pieces fit {#pieces}

Three inputs go into one event log per organization, and every figure is computed from the log as of a date.

<div class="diagram" aria-label="Agents send messages and telemetry; bills come from the provider; rules assign sessions to features; the event log gives reports as of a date"><svg viewBox="0 0 760 250" role="img"><defs><marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z" fill="#C6C1B4"></path></marker></defs><g font-family="Inter, sans-serif" font-size="13" fill="#141613"><rect x="10" y="20" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="43" text-anchor="middle" font-weight="600">Agents</text><text x="95" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">Claude Code, Codex, any</text> <rect x="10" y="98" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="121" text-anchor="middle" font-weight="600">Mod and CLI</text><text x="95" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">branch, PR, summary, review</text> <rect x="10" y="176" width="170" height="54" rx="8" fill="#FCFBF8" stroke="#DDD9CF"></rect><text x="95" y="199" text-anchor="middle" font-weight="600">Provider</text><text x="95" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">Anthropic, OpenAI, CSV</text> <rect x="260" y="20" width="150" height="54" rx="10" fill="#EFEBE3" stroke="#DDD9CF"></rect><text x="335" y="43" text-anchor="middle" font-weight="600">Telemetry</text><text x="335" y="61" text-anchor="middle" fill="#6B6E67" font-size="12">counts per model call</text> <rect x="260" y="98" width="150" height="54" rx="10" fill="#E2ECE5" stroke="#DDD9CF"></rect><text x="335" y="121" text-anchor="middle" font-weight="600">Messages</text><text x="335" y="139" text-anchor="middle" fill="#6B6E67" font-size="12">split by rules</text> <rect x="260" y="176" width="150" height="54" rx="10" fill="#F4E7CF" stroke="#DDD9CF"></rect><text x="335" y="199" text-anchor="middle" font-weight="600">Bills</text><text x="335" y="217" text-anchor="middle" fill="#6B6E67" font-size="12">the total to reconcile</text> <rect x="490" y="72" width="150" height="106" rx="12" fill="#fff" stroke="#1C2030" stroke-width="1.5"></rect><text x="565" y="112" text-anchor="middle" font-weight="600">Event log</text><text x="565" y="131" text-anchor="middle" fill="#6B6E67" font-size="12">per organization</text><text x="565" y="148" text-anchor="middle" fill="#6B6E67" font-size="12">read as of a date</text> <rect x="670" y="72" width="80" height="106" rx="8" fill="#1C2030"></rect><text x="710" y="119" text-anchor="middle" fill="#fff" font-weight="600" font-size="12">Reports</text><text x="710" y="137" text-anchor="middle" fill="#C6C1B4" font-size="11">cost, plan, Δ</text></g><g stroke="#C6C1B4" stroke-width="1.4" fill="none" marker-end="url(#ar)"><path d="M180,47 L258,47"></path><path d="M180,125 L258,125"></path><path d="M180,203 L258,203"></path><path d="M410,47 C450,47 450,100 488,100"></path><path d="M410,125 L488,125"></path><path d="M410,203 C450,203 450,150 488,150"></path><path d="M640,125 L668,125"></path></g></svg></div>

| Input | What it carries | Where it comes from |
| --- | --- | --- |
| [Telemetry](/collect/telemetry) | One message per model call: token counts by kind, model, session and message ids, person, repository. No text. | The agent's own OpenTelemetry exporter. Nothing is installed on laptops. |
| [Messages and sessions](/collect) | The same counts, plus the session facts: branch, pull request, summary, and the tags the person or the rules chose. | From a person's machine through the Claude Code mod or the CLI; from cloud, CI and support agents through the GitHub Action or the token entries endpoint. |
| [Bills](/account/bills) | The provider's figure for a date range: amount and currency. | Anthropic and OpenAI admin APIs, CSV upload, manual entry, or the API. |

Telemetry and a session file for the same message are one record, counted once. A bill equals the cost of its provider's messages in its range plus what no message explains. [How it works](/concepts) explains the ideas; every term is in the [glossary](/glossary).

## Quick start for people managers {#managers}

About fifteen minutes, once. To connect the bill you need an admin key from Anthropic or OpenAI, or the bill as CSV.

<ol class="steps">
  <li><b>Create the organization.</b><p>Sign up at <a href="https://app.tokencontroller.com/signup"><code>app.tokencontroller.com/signup</code></a>. See <a href="/account/organizations">Organization settings</a>.</p></li>
  <li><b>Check the price table.</b><p>The hosted service brings the managed price table. Add or override rows for prices you negotiated. See <a href="/account/price-table">Price table</a>.</p></li>
  <li><b>Connect the bill.</b><p>Under <em>Bills</em>, add an Anthropic Enterprise Analytics key, a Console admin key or an OpenAI admin key, or upload the bill as CSV. See <a href="/account/bills">Bills</a>.</p></li>
  <li><b>Create your tag values.</b><p>Under <em>Tags</em>, create projects with their client, connect Jira so features sync, and add rules such as "key <code>^KD-\d+</code> in the branch is that feature" and "repository <code>kestrel/shop</code> is project Shop". See <a href="/account/tags">Tags</a> and <a href="/attribute/rules">Rules</a>.</p></li>
  <li><b>Invite people.</b><p>Give each person their attributes, such as <code>team: web</code>, and a role. They install the mod or the CLI (<a href="#engineers">below</a>), or you cover everyone on Claude Code at once with <a href="/collect/telemetry">telemetry push</a>. See <a href="/account/attributes">Attributes</a> and <a href="/account/roles">Grants and roles</a>.</p></li>
  <li><b>Move the cutoff.</b><p>When a bill's range is reconciled and the submissions you want are in, move the cutoff to its last day. Figures up to it are frozen. See <a href="/account/cutoff">Cutoff</a>.</p></li>
</ol>

## Quick start for agent managers {#engineers}

Your sessions become messages as you work, and your organization's rules put them on features and projects. You check them and submit.

![The overview an agent manager sees: own cost this month, on features, to check, and the plan](/screens/02-overview-agent-manager.png)

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
/plugin install token-controller
```

After every turn the mod records Claude Code's messages, writes the one-line summary and splits the session by your rules. A band above the prompt shows what you spent and how much is on features. See [Claude Code mod](/collect/claude-code).

### Check and submit {#review}

```sh
tc review                   # your sessions and where they were put
tc tag 5 feature=KD-231     # fix one that a rule got wrong
tc submit                   # submit everything since your last submit
```

See [Check and submit](/attribute/review).

### Telemetry instead of an install {#telemetry}

If your administrator pushes [telemetry](/collect/telemetry), your messages are counted without any install, and they land on projects by repository. The mod or the CLI add the branch, the pull request and the summary, so cost lands on features.

## Quick start for the API {#api}

Everything the web app does is a call to the [HTTP API](/reference/api). Create a service token under _Settings_ and report a session from any agent:

```http
POST https://app.tokencontroller.com/api/v1/token-entries
Authorization: Bearer tc_svc_…

{
  "agent_harness": "support-desk",
  "model": "claude-sonnet-5",
  "started_at": "2026-03-04T09:12:00Z",
  "ended_at": "2026-03-04T09:14:10Z",
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200, "cache_write": 0 },
  "feature": "KD-1"
}
```

The answer carries its cost from the price table. A read token reads every [report](/reference/api/reports) and feeds the [MCP server](/reference/api/mcp); `POST /estimates` says what described work will [cost](/reference/api/estimates) before it starts.

## Quick start for self-hosters {#self-hosting}

<ol class="steps">
  <li><b>Start Postgres and the service.</b><p>One Docker image and one Postgres database. See <a href="/self-hosting">Self-hosting</a>.</p></li>
  <li><b>Set your public URL, email and price table.</b><p>The open edition ships without prices; bring your own file. See <a href="/self-hosting/configuration">Configuration</a>.</p></li>
  <li><b>Point the tools at it.</b><p><code>tc connect https://tc.example.com</code>, and telemetry at <code>https://tc.example.com/api/v1/ingest/otlp</code>. Then follow the <a href="#managers">quick start for people managers</a>.</p></li>
</ol>
