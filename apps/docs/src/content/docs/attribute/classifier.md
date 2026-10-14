---
title: "Classifier"
description: "An optional program on your machine that picks the ticket when rules cannot."
---

When rules cannot decide, an optional classifier on your own machine picks the ticket. It is any program you choose; Token Controller ships no model.

## How it works {#how}

For a session the rules could not place, or placed on a ticket that was only mentioned, the CLI calls your classifier with a short digest of the session and the candidate tickets. The classifier answers with one of them, or none. The digest never leaves your machine.

## Set it up {#setup}

```sh
tc config classifier "~/bin/pick-ticket"
```

## The reference script {#reference}

The CLI ships one reference script, `pick-ticket`, in every download, and in `$(brew --prefix)/share/tc/` with Homebrew. It uses Laya, an open local model under Apache-2.0. It needs Python 3.10 or later and `pip install laya`; the model downloads on first use. Use it as it is or as a starting point.

## You always have the last word {#override}

The classifier's pick shows in your [review list](/attribute/review), and you can place the token entry differently by hand.
