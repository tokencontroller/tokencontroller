---
title: "Security and privacy"
description: "What leaves a person's machine, what a message contains, and who can see it."
---

Token Controller stores numbers, identifiers, a free-text summary per session, tags and the estimate descriptions you send, never what an agent read or wrote.

## What never leaves the machine {#never}

Transcripts, prompts, tool output and file contents. Nothing in Token Controller reads them off the machine, and no door accepts them.

The [Estimates API](/reference/api#estimates) is the one place you send text on purpose: the description of the work and any context. Token Controller keeps it and trains its [System 1 classifier](/glossary#system-1-classifier) on it, together with the summaries of all organizations. The classifier answers with probabilities only.

## What a message contains {#token-entry}

This is the full list. The summary and the tag values are the only text. The agent writes the summary, and you see it and the tags before you submit. Tag values are the names your organization uses, such as a project or a client. Every field is in the [token entry schema](/reference/token-entry).

| Field | Example |
| --- | --- |
| Agent harness, model | claude-code, opus-5 |
| Agent name | review bot (optional) |
| Person | m.ortner@kestrel.example |
| Time, session id, message id | 09:12 UTC, one id each |
| Token counts by kind | input, output, cache read, cache write |
| Reported cost | the agent's own cost figure, kept as a fact |
| Session facts | turns, tool calls, retries, duration, subagents, files touched, result pointer |
| Signals | branch, working directory, repository, pull request number and title, subagent type, config file values, and keys found in prompts and titles (the keys only, never the text) |
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

## Telemetry {#telemetry}

[Telemetry push](/collect/telemetry) sends numbers and identifiers only. The ingest endpoint never stores prompt text: it removes it from model call events and drops any other event that carries it.

## Who sees what {#who-sees}

Everyone sees their own cost. A people manager sees cost per person inside the filters they were granted view on, such as a team or a client. An org manager sees everything. See [Grants and roles](/account/roles), and [Scope](/scope#out) for why there is no aggregate-only mode.

## Where data is stored {#hosting}

Solo and the Team plan are hosted in the EU. On the Enterprise plan you choose the region; self-hosted runs where you run it. See [Plans and limits](/plans#region).
