---
title: "Overview"
description: "What Token Controller is, how its pieces fit, and how to get started as a people manager, an agent manager, a self-hoster or on the API."
---

When your team works with AI agents, you may need to explain the AI bill: what each bill paid for, which project or client the work was for, and whether the bill matches what your agents used. Token Controller collects every model call your agents make as a message, prices it with a price table, and lets you check each bill against the messages under the same contract. You can start small, with only messages and one bill, and add tags, plans and approval later.

<div class="cards">
  <a class="card" href="#managers"><i></i><b>I manage people</b><span>Create the organization, bring in the bills, add tags, invite people.</span></a>
  <a class="card b" href="#engineers"><i></i><b>I manage agents</b><span>Install the Claude Code mod or the CLI, check your sessions, submit.</span></a>
  <a class="card s" href="#self-hosting"><i></i><b>I host it myself</b><span>One Docker image, one Postgres. The same features as the hosted Team plan.</span></a>
  <a class="card" href="#api"><i></i><b>I build on the API</b><span>Report usage from any agent, read every report, ask what work may cost.</span></a>
</div>

## How the pieces fit {#pieces}

Usage, tags and bills each come from their own sources, in any order. Tags go on messages and on bills afterwards, and the contract tag lets a bill be checked against its own messages. Every figure is computed from your organization's log, as of a date.

<div class="diagram flow"><svg viewBox="0 0 720 350" role="img" aria-label="The full picture: usage sources bring messages, tag sources bring tags, bill sources bring bills; tags go on messages and bills; the price table gives cost; reports show cost by any tag; the check compares each bill with the messages under its contract"><defs><marker id="ar-overview" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path class="ah" d="M0,0 L10,5 L0,10 z"/></marker></defs><rect class="src" x="0" y="20" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="42" text-anchor="middle">Usage sources</text><text class="sub" x="75.0" y="60" text-anchor="middle">collectors send to</text><text class="sub" x="75.0" y="76" text-anchor="middle">usage endpoints</text><rect class="rec" x="190" y="20" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="42" text-anchor="middle">Message</text><text class="sub" x="265.0" y="60" text-anchor="middle">tokens by kind</text><text class="sub" x="265.0" y="76" text-anchor="middle">signals</text><rect class="out" x="380" y="20" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="42" text-anchor="middle">Cost</text><text class="sub" x="455.0" y="60" text-anchor="middle">tokens × price</text><text class="sub" x="455.0" y="76" text-anchor="middle">per kind</text><rect class="out" x="570" y="20" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="42" text-anchor="middle">Reports</text><text class="sub" x="645.0" y="60" text-anchor="middle">cost by any tag</text><rect class="src" x="0" y="140" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="162" text-anchor="middle">Tag sources</text><text class="sub" x="75.0" y="180" text-anchor="middle">Jira · CRM · list</text><text class="sub" x="75.0" y="196" text-anchor="middle">by hand</text><rect class="tag" x="190" y="140" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="162" text-anchor="middle">Tags</text><text class="sub" x="265.0" y="180" text-anchor="middle">project · feature</text><text class="sub" x="265.0" y="196" text-anchor="middle">client · contract</text><rect class="rec" x="380" y="140" width="150" height="74" rx="8"/><text class="hd" x="455.0" y="162" text-anchor="middle">Price table</text><text class="sub" x="455.0" y="180" text-anchor="middle">ours, or the</text><text class="sub" x="455.0" y="196" text-anchor="middle">contract&#x27;s own</text><rect class="src" x="0" y="260" width="150" height="74" rx="8"/><text class="hd" x="75.0" y="282" text-anchor="middle">Bill sources</text><text class="sub" x="75.0" y="300" text-anchor="middle">billing APIs</text><text class="sub" x="75.0" y="316" text-anchor="middle">CSV upload</text><rect class="rec" x="190" y="260" width="150" height="74" rx="8"/><text class="hd" x="265.0" y="282" text-anchor="middle">Bill</text><text class="sub" x="265.0" y="300" text-anchor="middle">amount · dates</text><rect class="out" x="570" y="200" width="150" height="74" rx="8"/><text class="hd" x="645.0" y="222" text-anchor="middle">The check</text><text class="sub" x="645.0" y="240" text-anchor="middle">bill = messages</text><text class="sub" x="645.0" y="256" text-anchor="middle">+ unexplained</text><path class="ln" d="M150,57 L188,57" marker-end="url(#ar-overview)"/><path class="ln" d="M340,57 L378,57" marker-end="url(#ar-overview)"/><path class="ln" d="M530,57 L568,57" marker-end="url(#ar-overview)"/><path class="ln" d="M150,177 L188,177" marker-end="url(#ar-overview)"/><path class="ln" d="M150,297 L188,297" marker-end="url(#ar-overview)"/><path class="ln" d="M265,140 L265,96" marker-end="url(#ar-overview)"/><text class="lbl" x="290" y="122" text-anchor="middle">rules</text><path class="ln" d="M265,214 L265,258" marker-end="url(#ar-overview)"/><text class="lbl" x="298" y="240" text-anchor="middle">contract</text><path class="ln" d="M455,140 L455,96" marker-end="url(#ar-overview)"/><path class="ln" d="M530,80 L600,198" marker-end="url(#ar-overview)"/><path class="ln" d="M340,297 L568,250" marker-end="url(#ar-overview)"/></svg></div>

| Source | What it brings | Where it comes from |
| --- | --- | --- |
| [Usage](/collect) | Messages: one per model call, with person, agent, model, time and tokens by kind. Everything else that came along is kept as a signal. | A collector sends to a usage endpoint: [telemetry](/collect/telemetry) from the agent itself, the [Claude Code mod](/collect/claude-code) or the [CLI](/collect/cli) on a person's machine, the [GitHub Action](/collect/github-action) in CI, or the [token entries endpoint](/collect/token-entries) from any agent. |
| [Tags](/account/tags) | What the work was for: project, feature, client, contract, or a name of your own. | Jira, a list, or typed by hand. [Rules](/attribute/rules) put them on messages from their signals. |
| [Bills](/account/bills) | What a provider charged under one contract for a date range. | Anthropic and OpenAI admin APIs, a CSV upload, typed in, or the API. |

[How it works](/concepts) builds this picture step by step with one worked month, and every term is in the [glossary](/glossary).

## Quick start for people managers {#managers}

About fifteen minutes, once. To connect a bill you need an admin key from Anthropic or OpenAI, or the bill as CSV. A seat plan's bill can be typed in.

<ol class="steps">
  <li><b>Create the organization.</b><p>Sign up at <a href="https://app.tokencontroller.com/signup"><code>app.tokencontroller.com/signup</code></a>. See <a href="/account/organizations">Organization settings</a>.</p></li>
  <li><b>Look at the price table.</b><p>The hosted service brings the managed price table. You can add rows for prices you negotiated, for every contract or for one. See <a href="/account/price-table">Price table</a>.</p></li>
  <li><b>Bring in the bills.</b><p>Under <em>Bills</em>, connect an Anthropic Enterprise Analytics key, a Console admin key or an OpenAI admin key, upload a CSV, or add a bill by hand. Each bill creates its contract, so there is nothing to set up first. See <a href="/account/bills">Bills</a>.</p></li>
  <li><b>Create your tag values.</b><p>Under <em>Tags</em>, create projects with their client, connect Jira if features live there, and add rules such as "key <code>^KD-\d+</code> in the branch is that feature" and "repository <code>kestrel/shop</code> is project Shop". See <a href="/account/tags">Tags</a> and <a href="/attribute/rules">Rules</a>.</p></li>
  <li><b>Invite people.</b><p>Give each person their attributes, such as <code>team: web</code>, and a role. They install the mod or the CLI (<a href="#engineers">below</a>), or you cover everyone on Claude Code at once with <a href="/collect/telemetry">telemetry push</a>. See <a href="/account/attributes">Attributes</a> and <a href="/account/roles">Grants and roles</a>.</p></li>
  <li><b>Move the cutoff.</b><p>When the bills of a date range are checked and the submissions you want are in, you may move the cutoff to its last day. Figures up to it are frozen. See <a href="/account/cutoff">Cutoff</a>.</p></li>
</ol>

## Quick start for agent managers {#engineers}

Your sessions become messages as you work, and your organization's rules put them on features and projects. You look them over, fix what a rule got wrong, and submit.

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

After every turn the mod hands the session to `tc`, which reads its messages and splits it by your rules. The mod writes a one-line summary, and a band above the prompt shows what you spent and how much is on features. See [Claude Code mod](/collect/claude-code).

### Check and submit {#review}

```sh
tc review                   # your sessions and where they were put
tc tag 5 feature=KD-231     # fix one that a rule got wrong
tc submit                   # submit everything since your last submit
```

See [Check and submit](/attribute/review).

### Telemetry instead of an install {#telemetry}

If your administrator pushes [telemetry](/collect/telemetry), your messages are counted without any install, and rules can put them on projects by repository. The mod or the CLI add the branch, the pull request and the summary, so cost can land on features.

## Quick start for the API {#api}

Everything the web app does is a call to the [HTTP API](/reference/api). Create a service token under _Settings_ and report a session from any agent:

```http
POST https://app.tokencontroller.com/api/v1/token-entries
Authorization: Bearer tc_svc_…

{
  "agent_harness": "support-desk",
  "provider": "anthropic",
  "model": "claude-sonnet-5",
  "started_at": "2026-03-04T09:12:00Z",
  "ended_at": "2026-03-04T09:14:10Z",
  "tokens": { "input": 4120, "output": 610, "cache_read": 18200 },
  "feature": "KD-1"
}
```

The answer carries the session's cost from the price table. A read token reads every [report](/reference/api/reports) and feeds the [MCP server](/reference/api/mcp); `POST /estimates` says what described work may [cost](/reference/api/estimates) before it starts.

## Quick start for self-hosters {#self-hosting}

<ol class="steps">
  <li><b>Start Postgres and the service.</b><p>One Docker image and one Postgres database. See <a href="/self-hosting">Self-hosting</a>.</p></li>
  <li><b>Set your public URL, email and price table.</b><p>The open edition ships without prices; bring your own file. See <a href="/self-hosting/configuration">Configuration</a>.</p></li>
  <li><b>Point the tools at it.</b><p><code>tc connect https://tc.example.com</code>, and telemetry at <code>https://tc.example.com/api/v1/ingest/otlp</code>. Then follow the <a href="#managers">quick start for people managers</a>.</p></li>
</ol>
