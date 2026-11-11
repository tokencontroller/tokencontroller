---
title: "Security and privacy"
description: "What leaves a person's machine, what a message contains, what the usage endpoints store and for how long, and who can see it."
---

Before you roll out a collector, you, your team or a works council may want to know what leaves each machine and who can read it. By default Token Controller stores numbers, identifiers, signals, a free-text summary per session, tags and the estimate descriptions you send, and not what an agent read or wrote.

## Content text is off by default {#never}

Content text is prompts, responses, tool output and file contents. The collectors Token Controller ships send none of it: `tc` reads prompts on the machine only to find feature keys, and sends the keys. Every usage endpoint removes content text on arrival, unless your organization switched keeping it on. Some agents send it in their telemetry by default, such as Gemini CLI; it is removed all the same.

An org manager switches keeping it on with `keep_content_text` on the [organization](/reference/api/organization); there is no switch for it in the web app. Kept content text stays in the received records only, is shown on no screen, and is deleted with them.

The [Estimates API](/reference/api/estimates) is the one place you send text on purpose: the description of the work and any context. Token Controller keeps it and trains its [System 1 classifier](/glossary#system-1-classifier) on it, together with the summaries of all organizations. The classifier answers with probabilities only.

## What a message contains {#token-entry}

This is the full list with content text off. The summary, a pull request title and the tag values are the only free text. The agent writes the summary, and you see it and the tags before you submit. Tag values are the names your organization uses, such as a project or a client. Every field is in the [token entry schema](/reference/token-entry).

| Field | Example |
| --- | --- |
| Agent, model, provider when the agent reports it | claude-code, claude-opus-5, anthropic |
| Agent name | review bot (optional) |
| Person | m.ortner@kestrel.example |
| Time, session id, message id | 09:12 UTC, one id each |
| Token counts by kind | input, output, cache read, cache write |
| Signals | Anything else the collector sent: branch, working directory, repository, pull request number and title, how the agent was signed in, the agent's own cost figure, session facts such as turns and tool calls, config file values, and feature keys found in prompts (the keys only, never the prompt) |
| Summary | Fixed checkout redirect loop |
| Tags | feature KD-214, project shop |

## What a feedback report contains {#feedback}

On the hosted service, the feedback button in the web app sends a bug, an idea or a question. You see the whole report before you send it. A self-hosted instance has no feedback button.

| Part | What it holds |
| --- | --- |
| Kind | Bug, idea or question |
| Text | What you write |
| Screenshot | Only when you turn it on. You see it first and can blur any part of it. It is deleted after 90 days. |
| Diagnostics | The page's path without anything after `?`, the app version, the browser, the screen size, the last error messages the app showed, and the IDs of the last requests. Never what a request contained. |

A report goes to us and is stored in the EU. It is deleted two years after its last change. Your reports and their answers are under Feedback in the web app. When one is answered or fixed, you get an email with a link, never the answer itself.

## What the usage endpoints store {#telemetry}

Every usage endpoint, the [OpenTelemetry endpoint](/collect/telemetry), the [token entries endpoint](/collect/token-entries) and the [CSV import](/collect/manual), stores what it received as a received record before it reads it, without content text unless your organization keeps it. Messages and activities are made from it and point back to it, so a field Token Controller learns to read later can still count for the past.

| | |
| --- | --- |
| Kept for | 400 days, then deleted |
| Who can list and delete them | Org managers, through the [API](/reference/api/messages#received-records). Deleting one changes no figure: the messages made from it stay. |
| Outside the log | Received records are not part of the organization's append-only log, so a delete is a real delete. |

## Who sees what {#who-sees}

Everyone sees their own cost. A people manager sees cost per person inside the filters they were granted view on, such as a team or a client. An org manager sees everything. See [Grants and roles](/account/roles), and [Scope](/scope#out) for why there is no aggregate-only mode.

## Where data is stored {#hosting}

Solo and the Team plan are hosted in the EU. On the Enterprise plan you choose the region; self-hosted runs where you run it. See [Plans and limits](/plans#region).
