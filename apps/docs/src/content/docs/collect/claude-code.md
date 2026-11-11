---
title: "Claude Code mod"
description: "Install the Claude Code mod, what it records and sends, and what it shows in the terminal and the desktop app."
---

If you work in Claude Code, you may want to see what a session costs and where it went without leaving it. The Claude Code mod records what Claude Code spends, message by message, and shows it beside the session.

## Install {#install}

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-controller
```

The mod keeps its messages through the CLI, so [install `tc`](/collect/cli#install) too. Then [connect it](/collect/cli#connect) to your organization, or leave it local. Until `tc` is installed, the mod keeps the session's summary and tags on the machine, and the band says how to install it.

## Where it runs {#where}

| Where Claude Code runs | What the mod does |
| --- | --- |
| Terminal on macOS, Windows, Linux | Records and shows everything below. |
| Claude desktop app, Code tab | Records and shows everything below. |
| VS Code panel, CI, cloud sessions | Records messages; shows nothing. |

Nothing shows while no Claude Code session is open. People who use other agents use the CLI and the web app.

## What it records {#records}

After every turn the mod has `tc` read Claude Code's session files: every message with its token counts by kind and model, the working directory, the branch, and feature keys in your prompts. `tc` builds the session and splits it by your [rules](/attribute/rules); the mod splits nothing itself. The mod writes the summary during the session, with a small model call of its own, so the summary exists even if the session is closed abruptly. It also counts the session facts: turns, their duration and subagent turns.

## What it sends {#sends}

| | What goes to your organization |
| --- | --- |
| Messages | Model, time and token counts by kind, through `tc`. No provider: Claude Code's session file names none, so a message is priced when every provider in the price table charges the same for its model, or once it has a contract. |
| How Claude Code was signed in | Recorded once, at session start, as the signals `sign_in` and, for a claude.ai or Console sign-in, `organization.id`. A later sign-in never lands on an earlier session. Claude Code tells this only to a mod that managed settings install; otherwise `tc` reads the organization Claude Code wrote into the session file at session start. With an API key there is no organization. |
| Summary, facts, tags | The one-line summary, the session facts, and the feature and tags you set by hand. |
| Content text | None. Only the feature keys leave a prompt, never its text. |

With the signed-in organization, a shipped sign-in rule puts the session's messages under that organization's own contract, so a seat plan's messages are told apart from an API account's. See [which contract a message gets](/reference/data-model#contract).

## What it shows {#shows}

- **A band above the prompt** with the headline number, the share on features, and a note when an org manager asks everyone to submit before moving the cutoff.
- **A pane** (`/tc`) beside the session: the review list, where you fix a feature and submit; a report by feature, project, any tag or day; and your personal rules.

## Commands {#commands}

```sh
/tc feature KD-214     # put this session on a feature
/tc tag purpose=spike  # set any other tag
/tc report             # a report table, right in the session
/tc submit             # submit everything since your last submit
/tc                    # open the pane
```

These run at once, without a turn. You can also ask Claude, for example "what did KD-214 cost this week?", and the mod answers with a report table.
