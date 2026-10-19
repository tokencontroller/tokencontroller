---
title: "Claude Code mod"
description: "Install the Claude Code mod, what it records, and what it shows in the terminal and the desktop app."
---

The Claude Code mod records what Claude Code spends, message by message, and shows where it went without leaving the session.

## Install {#install}

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-controller
```

The mod keeps its messages through the CLI, so [install `tc`](/collect/cli#install) too. Then [connect it](/collect/cli#connect) to your organization, or leave it local.

## Where it runs {#where}

| Where Claude Code runs | What the mod does |
| --- | --- |
| Terminal on macOS, Windows, Linux | Records and shows everything below. |
| Claude desktop app, Code tab | Records and shows everything below. |
| VS Code panel, CI, cloud sessions | Records messages; shows nothing. |

Nothing shows while no Claude Code session is open. People who use other agents use the CLI and the web app.

## What it records {#records}

After every turn the mod has the CLI read Claude Code's session files: every message with its token counts by kind and model, and the session facts. The mod writes the summary during the session, with a small model call of its own, so the summary exists even if the session is closed abruptly. Rules split the session on arrival; see [Rules](/attribute/rules).

## What it shows {#shows}

- **A band above the prompt** with the headline number, the share on features, and a note when an org manager asks everyone to submit before moving the cutoff.
- **A pane** beside the conversation with small charts and filters, the check list, submit, and rule editing.

## Commands {#commands}

```sh
/tc feature KD-214     # put this session on a feature
/tc tag purpose=spike  # set any other tag
/tc report             # a report table, right in the session
/tc submit             # submit everything since your last submit
```

You can also ask Claude, for example "what did KD-214 cost this week?", and the mod answers with a report table.
