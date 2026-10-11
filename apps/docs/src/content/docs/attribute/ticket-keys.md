---
title: "Ticket key patterns"
description: "Tell Token Controller how a project's tickets are named."
---

A ticket key pattern tells Token Controller how a project's tickets are named, so it can find them in branches, pull requests and prompts.

## Set the pattern {#pattern}

Each project has one pattern, a regular expression. For Jira-style keys:

```text
^(KD|OPS)-\d+
```

For GitHub Issues, where tickets are numbers, use the repository with the number, such as `storefront#\d+`.

## Where keys are found {#where}

In the branch name, in the pull request title, in the session title and in your prompts. Only the keys leave the machine, never the prompt.

The Claude Code mod, the CLI and the GitHub Action need only the patterns to find keys. They never need your tickets. Token Controller checks every key against the tickets it syncs from your ticket system.

## Keys for tickets Token Controller does not know {#unknown}

If a key matches the pattern but the ticket has not synced yet, the token entry is placed on it anyway, and the ticket fills in at the next sync. If the next sync still does not find it, the token entry shows in the review list as _unknown ticket_, and rules and the summary match run again without that key.
