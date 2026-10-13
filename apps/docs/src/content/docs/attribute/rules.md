---
title: "Rules"
description: "How rules turn signals into postings, in which order, and what happens when nothing matches."
---

Rules set the tags of token entries: their ticket and project, and any other tag, such as `purpose: code-review` for sessions on a `review/` branch. They work like the rules in a banking app: set once, every new token entry lands where it belongs, and people only handle the exceptions.

![Rules: the organization rules in order, personal rules and the summary match](/screens/14-rules.png)

## Signals {#signals}

A signal is a fact about a session that hints at where its cost belongs:

- the branch and the working directory
- the repository
- the pull request number
- the subagent type
- ticket keys that match a project's [ticket key pattern](/attribute/ticket-keys), found in your prompts, the session title or the pull request title

Prompts and titles are read on the machine. Only the ticket keys found in them leave it, as [Security and privacy](/security) lists.

## Where rules run {#where}

Rules run twice.

- **On the machine.** The Claude Code mod, the CLI and the GitHub Action know your rules and ticket key patterns, but not your tickets. They read the signals, including prompts and titles that never leave the machine, and send each token entry with the ticket key they found.
- **In Token Controller.** The service knows your tickets from your ticket system. It checks every ticket key against them, runs the rules again on the signals it received, and tries a summary match for token entries that still have no ticket.

Token entries from the token entries endpoint or a CSV import only go through the second pass.

## The default rules {#defaults}

| Rule | Sets |
| --- | --- |
| Ticket key in branch | That ticket |
| Ticket key in pull request title | That ticket |
| Ticket key in prompts or session title | That ticket |
| Repository mapped to a project | That project, as project overhead |

Add your own in the web app under _Rules_, in the mod's pane, or with the CLI. A rule can set any tag on your organization's list, such as `purpose: code-review` for branches that start with `review/`.

## Order {#order}

This order decides the ticket and the project. Each token entry goes through these steps, and the first that finds a place wins:

1. **A ticket named in the token entry.** Picked by hand in the mod or the CLI, passed to the [GitHub Action](/collect/github-action), or sent in the `ticket` field.
2. **Rules that find a ticket.** By default: the ticket key in the branch, then in the pull request title, then in your prompts or the session title.
3. **Summary match.** See [below](#summary).
4. **Rules that place on a project**, such as the repository rule.
5. **Nothing.** See [below](#nothing).

Within a step, organization rules run before personal ones, so a person's own rules only handle what the organization's rules leave open. Rules for other tags all run; when two set the same tag name, the organization rule wins, then the one listed first. A tag sent with the token entry, or set by hand, wins over every rule.

## When prompts name several tickets {#several-keys}

By default the ticket named most often in your prompts wins, and on a tie the latest one. An organization can switch this to an equal split between the tickets, or to the first one named.

<div class="note"><b>A mention is not work.</b> "The follow-up ticket will be KD-260" names a ticket the session did not work on. Rules cannot tell the difference. The <a href="/attribute/classifier">classifier</a> can, and you can always place a token entry by hand.</div>

## Summary match {#summary}

When no rule finds a ticket, Token Controller compares the token entry's summary with the titles of open tickets, in the project the repository maps to or, without one, in the whole organization. If one ticket clearly fits, the token entry is placed on it and marked _matched by summary_ in the review list and in reports. If none clearly fits, the next step runs. An organization can switch summary matching off under _Rules_.

## When rules change {#changes}

A new or changed rule runs again over the token entries of every open period, so a fix also reaches past work. Token entries placed by hand keep their place. Closed periods are locked and never change; correct them with an [adjustment](/account/adjustments).

## When nothing matches {#nothing}

The token entry stays organization overhead with its person, and shows up in your review list so you can add a ticket or project if it had one. See [cost kinds](/account/tags#kinds).
