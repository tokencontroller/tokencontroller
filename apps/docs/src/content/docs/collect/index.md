---
title: "Choosing a door"
description: "The ways token entries and telemetry reach Token Controller, by agent harness, and which to start with."
---

A door is a way token entries or telemetry reach Token Controller. Most organizations use two: telemetry for everyone, and the mod or the CLI for people who want cost on tickets.

## Doors by agent harness {#by-agent}

| Agent harness | Doors |
| --- | --- |
| Claude Code | The [Claude Code mod](/collect/claude-code), session files through the [CLI](/collect/cli), [telemetry push](/collect/telemetry#claude-code), the [GitHub Action](/collect/github-action) in CI. |
| Codex CLI | [Telemetry push](/collect/telemetry#codex), session files through the CLI, the GitHub Action in CI. |
| Gemini CLI, Copilot CLI, Cursor Enterprise | [Telemetry push](/collect/telemetry#settings). |
| Any other agent | The [token entries endpoint](/collect/token-entries): one call per session. |
| Usage you only have on paper | [Manual entry and CSV import](/collect/manual) in the web app. |

## Which door to start with {#which}

| Door | Who installs what | Where cost lands |
| --- | --- | --- |
| Telemetry push | An administrator sets it once; nobody installs anything. | On projects, by repository. |
| The mod or the CLI | Each person installs it. | On tickets, from the branch, the pull request and the summary, which telemetry cannot see. Agent managers review before they submit. |
| The GitHub Action, the token entries endpoint | Set up once per agent. | Where the call or the job names it, else by your rules. For agents that run without a person: CI, cloud and support agents. |

Telemetry and the mod can run side by side: a session they both report is [counted once](/collect/telemetry#sees).

## Automatic token entries {#automatic}

Token entries from the GitHub Action and the token entries endpoint, and those built from telemetry when no token sheet claims the session, are automatic: they arrive with the status submitted and never go on a token sheet. The agent manager of an automation is the person who created its service token.
