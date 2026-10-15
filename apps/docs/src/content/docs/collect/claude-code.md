---
title: "Claude Code mod"
description: "Install the Claude Code mod, what it records, and what it shows in the terminal and the desktop app."
---

The Claude Code mod records what Claude Code spends, turn by turn, and shows where it went without leaving the session.

## Install {#install}

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-controller
```

The mod keeps its token entries through the CLI, so [install `tc`](/collect/cli#install) too. Then [connect it](/collect/cli#connect) to your organization, or leave it local.

## Where it runs {#where}

| Where Claude Code runs | What the mod does |
| --- | --- |
| Terminal on macOS, Windows, Linux | Records and shows everything below. |
| Claude desktop app, Code tab | Records and shows everything below. |
| VS Code panel, CI, cloud sessions | Records token entries; shows nothing. |

Nothing shows while no Claude Code session is open. People who use other agents use the CLI and the web app.

## What it records {#records}

After every turn the mod has the CLI read Claude Code's own usage figures from its session files: tokens by kind and the model. Together they make the session's draft token entry. The mod writes the summary during the session, with a small model call of its own, so the summary exists even if the session is closed abruptly. A session is split into segments when the branch or working directory changes; see [Sessions and subagents](/attribute/rules#sessions).

## What it shows {#shows}

- **A band above the prompt** with the headline number, the share on tickets, and a note when your token sheet is late or the org manager asks everyone to submit before the close.
- **A pane** beside the conversation with small charts and filters, the review list, submit, and rule editing.

## Commands {#commands}

```sh
/tc ticket KD-214     # set this session's ticket
/tc tag purpose=spike # set any other tag
/tc report            # a report table, right in the session
/tc submit            # submit your token sheet
```

You can also ask Claude, for example "what did KD-214 cost this week?", and the mod answers with a report table.
