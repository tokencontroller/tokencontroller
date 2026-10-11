---
title: "Security and privacy"
description: "What leaves a person's machine, what a token entry contains, and who can see it."
prev:
  link: "/reference/open-source"
  label: "Open source and licenses"
next:
  link: "/faq"
  label: "FAQ"
---

What leaves a person's machine, what Token Controller stores, and who can see it.

## What never leaves the machine {#never}

Transcripts, prompts, tool output and file contents. Nothing in Token Controller reads them off the machine, and no door accepts them.

## What a token entry contains {#token-entry}

This is the full list. The summary and the tag values are the only text. The agent writes the summary, and on a token sheet you see it and the tags before you submit. Tag values are the names your organization uses, such as a team or a client.

| Field | Example |
| --- | --- |
| Agent harness, model | claude-code, opus-5 |
| Agent name | review bot (optional) |
| Agent manager | m.ortner@kestrel.example |
| Start and end time | 09:12 and 11:40 UTC |
| Tokens by kind | input, output, cache write, cache read |
| Reported cost | the agent's own cost figure, where it has one |
| Signals | branch, working directory, repository, pull request number, subagent type, and ticket keys found in prompts, the session title and the pull request title (the keys only, never the text) |
| Summary | Fixed checkout redirect loop |
| Tags | ticket KD-214, project storefront, kind direct, team web |

## Telemetry {#telemetry}

Telemetry push sends numbers and identifiers only, with the exact settings shown in the [quick start](/#telemetry). The ingest endpoint drops any event that carries prompt text.

## Who sees what {#who-sees}

An agent manager sees their own cost. A people manager sees cost per person for the filters they are assigned, such as a team or a client. A controller sees everything. See [Roles](/account/roles).

## Where data is stored {#hosting}

Solo and the Team plan are hosted in the EU. On the Enterprise plan you choose the region; self-hosted runs where you run it. See [Plans and limits](/plans).

## Where to next {#next}
