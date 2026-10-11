---
title: "Sessions and subagents"
description: "Segments, measurement per message, and how subagents are placed."
---

One session is one token entry. Long sessions move between branches, so they are split into segments, each with its own tags, and subagents are tagged by their own signals.

## Segments {#segments}

A session that changes branch or working directory is split into segments. Each segment becomes a [split](/account/tags#splits) with its own tags, so one morning's session can land on two tickets.

## Measured per message {#grain}

Cost is measured per message, not per session. That is how segments split exactly, and how telemetry and bills join token entries: by message and request IDs.

## Subagents {#subagents}

A subagent is placed on its own signals when it has them, such as a review subagent on a pull request. Otherwise it follows the session that started it.
