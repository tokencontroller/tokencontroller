---
title: "Choosing a collector"
description: "The ways usage reaches Token Controller, by agent, what every message carries, and which collector to start with."
---

Your agents' usage may reach Token Controller in more than one way. A collector gathers it where the agents run and sends it to a usage endpoint: the OpenTelemetry endpoint, the token entries API or a CSV upload. Many organizations use two: telemetry for everyone, and the mod or the CLI for people who want cost on features.

## What every message must carry {#message}

Whatever the collector, a message is one model call: person, agent, model, time and token counts by kind, such as input, output, cache read and cache write. Kinds are open names; see [token kinds](/reference/data-model#token-kinds). A message without counts is a count error and has no cost, never zero. The provider comes along when the agent or the sender knows it; it is never guessed from the model's name. Everything else that came with the message, such as an agent's own cost figure, is kept as a signal and is never the figure shown.

A session adds its facts: turns, tool calls, retries and reruns, duration, subagents, files touched, a result pointer such as a pull request URL, and a summary. Content text such as prompts is removed unless your organization switches keeping it on; see [Security and privacy](/security).

## Collectors by agent {#by-agent}

| Agent | Collectors |
| --- | --- |
| Claude Code | The [Claude Code mod](/collect/claude-code), session files through the [CLI](/collect/cli), [telemetry push](/collect/telemetry#claude-code), the [GitHub Action](/collect/github-action) in CI. |
| Codex CLI | [Telemetry push](/collect/telemetry#codex), session files through the CLI, the GitHub Action in CI. |
| Gemini CLI, Copilot CLI, Cursor Enterprise | [Telemetry push](/collect/telemetry#settings). |
| Any other agent | The [token entries endpoint](/collect/token-entries): a token entry is one message or one session record. |
| Usage you only have on paper | [Manual entry and CSV import](/collect/manual) in the web app. |

## Which collector to start with {#which}

| Collector | Who installs what | Where cost lands |
| --- | --- | --- |
| Telemetry push | An administrator sets it once; nobody installs anything. | On projects, by repository. |
| The mod or the CLI | Each person installs it. | On features, from the branch, the pull request and the summary, which telemetry cannot see. People check before they submit. |
| The GitHub Action, the token entries endpoint | Set up once per agent. | Where the call or the job names it, else by your rules. For agents that run without a person: CI, cloud and support agents. |

Telemetry and the mod can run side by side: a message they both report is one record, [counted once](/collect/telemetry#sees).

## Messages from a service token {#service-token}

Messages from the GitHub Action and the token entries endpoint carry the person who created the service token. They are never submitted by hand and are approved like any other, when approval is on.
