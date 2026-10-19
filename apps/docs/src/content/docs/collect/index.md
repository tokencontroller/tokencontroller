---
title: "Choosing a door"
description: "The ways messages reach Token Controller, by agent harness, what every message must carry, and which door to start with."
---

A door is a way messages reach Token Controller. Most organizations use two: telemetry for everyone, and the mod or the CLI for people who want cost on features.

## What every message must carry {#message}

Whatever the door, a message is one model call with token counts by kind: input, output, cache read and cache write. A message without counts is a count error and has no cost, never zero. With the counts: person, provider, model, time, session id, message id, agent harness, and an optional agent name. An agent's own cost figure may come along as a fact; it is never the figure shown.

A session adds its facts: turns, tool calls, retries and reruns, duration, subagents, files touched, a result pointer such as a pull request URL, and a summary. Never prompts, never transcripts; see [Security and privacy](/security).

## Doors by agent harness {#by-agent}

| Agent harness | Doors |
| --- | --- |
| Claude Code | The [Claude Code mod](/collect/claude-code), session files through the [CLI](/collect/cli), [telemetry push](/collect/telemetry#claude-code), the [GitHub Action](/collect/github-action) in CI. |
| Codex CLI | [Telemetry push](/collect/telemetry#codex), session files through the CLI, the GitHub Action in CI. |
| Gemini CLI, Copilot CLI, Cursor Enterprise | [Telemetry push](/collect/telemetry#settings). |
| Any other agent | The [token entries endpoint](/collect/token-entries): a token entry is one message or one session record. |
| Usage you only have on paper | [Manual entry and CSV import](/collect/manual) in the web app. |

## Which door to start with {#which}

| Door | Who installs what | Where cost lands |
| --- | --- | --- |
| Telemetry push | An administrator sets it once; nobody installs anything. | On projects, by repository. |
| The mod or the CLI | Each person installs it. | On features, from the branch, the pull request and the summary, which telemetry cannot see. People check before they submit. |
| The GitHub Action, the token entries endpoint | Set up once per agent. | Where the call or the job names it, else by your rules. For agents that run without a person: CI, cloud and support agents. |

Telemetry and the mod can run side by side: a message they both report is one record, [counted once](/collect/telemetry#sees).

## Messages from a service token {#service-token}

Messages from the GitHub Action and the token entries endpoint carry the person who created the service token. They are never submitted by hand and are approved like any other, when approval is on.
