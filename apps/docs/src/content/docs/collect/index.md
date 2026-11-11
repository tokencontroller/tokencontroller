---
title: "Choosing a collector"
description: "The ways usage reaches Token Controller, by agent, what each collector sends, what the usage endpoints keep, and which collector to start with."
---

Your agents' usage may reach Token Controller in more than one way. A collector gathers it where the agents run and sends it to a usage endpoint: the OpenTelemetry endpoint, the token entries API or the CSV import. Many organizations use two: telemetry for everyone, and the mod or the CLI for people who want cost on features.

## What every message carries {#message}

Whatever the collector, a message is one model call: person, agent, model, time and token counts by kind, such as input, output, cache read and cache write. Kinds are open names; see [token kinds](/reference/data-model#token-kinds). A message without counts is a count error and has no cost, never zero.

The provider comes along only when the agent reports it or the sender knows it; it is never guessed from the model's name. A message without a provider is priced when every provider in the price table charges the same for its model, and is otherwise [not priced yet](/account/price-table#missing) until it gets a contract. Everything else that came with the message, such as the branch or an agent's own cost figure, is kept as a [signal](/reference/data-model#signals) and is never the figure shown.

A session adds its facts: turns, tool calls, retries and reruns, duration, subagents, files touched, a result pointer such as a pull request URL, and a summary.

## Collectors by agent {#by-agent}

| Agent | Collectors |
| --- | --- |
| Claude Code | The [Claude Code mod](/collect/claude-code), session files through the [CLI](/collect/cli), [telemetry push](/collect/telemetry#claude-code), the [GitHub Action](/collect/github-action) in CI. |
| Codex CLI | [Telemetry push](/collect/telemetry#codex), session files through the CLI, the GitHub Action in CI. |
| Gemini CLI, Copilot CLI, Cursor Enterprise | [Telemetry push](/collect/telemetry#settings). |
| Any other agent | The [token entries endpoint](/collect/token-entries): a token entry is one message or one session record. |
| Usage you only have on paper | The [CSV import](/collect/manual). |

## What each collector sends {#sends}

| Collector | Provider | How the agent was signed in | Content text |
| --- | --- | --- | --- |
| The mod and `tc`, Claude Code | None: Claude Code's session file names no provider. | The organization of a claude.ai or Console sign-in, as Claude Code recorded it at session start. Nothing for an API key. | None. Feature keys found in prompts, never the prompt. |
| `tc`, Codex | The provider Codex reports, such as `openai`. | Nothing. | None. |
| The GitHub Action | As `tc`, from the session files the job wrote. | As `tc`. | None. It adds the repository, the branch, and the pull request's number and title. |
| Telemetry | What the agent reports: Claude Code names its route (Anthropic, Bedrock, Vertex or Foundry), Gemini CLI its sign-in, Copilot CLI its provider. Codex and Cursor send none. | Claude Code sends its organization the same way. | Whatever the agent sends; removed on arrival unless your organization keeps it. |
| Your own script | What you send as `provider`. | What you send as a signal. | What you send; removed on arrival unless your organization keeps it. |

The signed-in organization arrives as the signal `organization.id`. A sign-in rule that ships with Token Controller puts those messages under that organization's own contract, and creates the contract the first time it matches. See [which contract a message gets](/reference/data-model#contract).

## What the usage endpoints keep {#received}

Every usage endpoint stores what it received as a received record before it reads it, with the endpoint and the time. Content text (prompts, responses, tool output, file contents) is removed first, unless your organization switched keeping it on. Messages and activities are made from the received record and point back to it. Received records are kept for 400 days, and org managers can delete them. See [Security and privacy](/security#telemetry).

## Which collector to start with {#which}

| Collector | Who installs what | Where cost lands |
| --- | --- | --- |
| Telemetry push | An administrator sets it once; nobody installs anything. | On projects, by a rule on the repository. |
| The mod or the CLI | Each person installs it. | On features, from the branch, the pull request and the summary, which telemetry cannot see. People check before they submit. |
| The GitHub Action, the token entries endpoint | Set up once per agent. | Where the call or the job names it, else by your rules. For agents that run without a person: CI, cloud and support agents. |

Telemetry and the mod can run side by side: a model call they both report counts [once](/collect/telemetry#sees).

## Messages from a service token {#service-token}

Messages from the GitHub Action and the token entries endpoint carry the person who created the service token. They are never submitted by hand and are approved like any other, when approval is on.
