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

One reference script ships with the CLI. It uses Laya, an open local model under Apache-2.0. Use it as it is or as a starting point. It is `pick-ticket` in every CLI download, and in `$(brew --prefix)/share/tc/` with Homebrew. It needs Python 3.10 or later and `pip install laya`; the model downloads on first use.

## You always have the last word {#override}

Whatever the classifier picks shows in your review list, and you can place it differently by hand.
