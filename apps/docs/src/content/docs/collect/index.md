---
title: "Doors"
description: "The ways token entries and telemetry reach Token Controller, by agent harness."
---

A door is a way token entries or telemetry reach Token Controller. Pick the ones that fit how your people and agents work; most organizations use two.

## Doors by agent harness {#by-agent}

| Agent harness | Doors |
| --- | --- |
| Claude Code | The [Claude Code mod](/collect/claude-code), session files through the [CLI](/collect/cli), [telemetry push](/collect/telemetry), the [GitHub Action](/collect/github-action) in CI. |
| Codex CLI | Telemetry push, and session files through the CLI. See [Codex CLI](/collect/codex). |
| Gemini CLI, Copilot CLI, Cursor Enterprise | Telemetry push. |
| Any other agent | The [token entries endpoint](/collect/token-entries): one call per session. |
| Usage you only have on paper | [Manual entry and CSV import](/collect/manual) in the web app. |

## Which door to start with {#which}

**Telemetry push** is the fastest way to cover a whole organization: an administrator sets it once in managed settings and nobody installs anything. It knows the person and the repository, so cost lands on projects.

**The mod and the CLI** add what telemetry cannot see: the branch, the pull request and the summary. With them, cost lands on tickets, and agent managers review their token entries before they submit.

**The GitHub Action and the token entries endpoint** are for agents that run without a person: CI, cloud and support agents. Their token entries arrive automatically and never go on a token sheet.

## Where file readers come from {#sources}

The CLI reads session files the way the open source ccusage project counts them. Token Controller reads session files for Claude Code and Codex only; every other agent reports by telemetry or through the endpoint.
