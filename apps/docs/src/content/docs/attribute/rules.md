---
title: "Rules"
description: "How rules place token entries on tickets and projects, in which order, what happens when nothing matches, and how sessions and subagents are split."
---

Rules set the tags of token entries: their ticket and project, and any other tag, such as `purpose: code-review` for sessions on a `review/` branch. Set them once, and people only fix the exceptions.

![Rules: the organization rules in order, personal rules and the summary match](/screens/14-rules.png)

## Signals {#signals}

A signal is a fact about a session that hints at where its cost belongs:

- the branch and the working directory
- the repository
- the pull request number
- the subagent type
- ticket keys that match a project's [ticket key pattern](/account/projects#ticket-keys), found in the branch, the pull request title, the session title or your prompts

Prompts and titles are read on the machine. Only the ticket keys found in them leave it, as [Security and privacy](/security) lists.

## Where rules run {#where}

Rules run twice. **On the machine**, the Claude Code mod, the CLI and the GitHub Action apply your rules and ticket key patterns, and send each token entry with the ticket key they found. **In Token Controller**, the service checks every key against your synced tickets, runs the rules again on the signals it received, and tries a [summary match](#summary). Token entries from the token entries endpoint or a CSV import only get the second pass.

## The default rules {#defaults}

| Rule | Sets |
| --- | --- |
| Ticket key in branch | That ticket |
| Ticket key in pull request title | That ticket |
| Ticket key in prompts or session title | That ticket |
| Repository mapped to a project | That project, as project overhead |

Add your own in the web app under _Rules_, in the mod's pane, or with `tc rules`. A rule can set any tag on your organization's list.

## Order {#order}

For the ticket and project, the first step that finds a place wins:

1. **A ticket named in the token entry.** Picked by hand in the mod or the CLI, passed to the [GitHub Action](/collect/github-action), or sent in the `ticket` field.
2. **Rules that find a ticket**, by default in the order of the table above.
3. **[Summary match](#summary).**
4. **Rules that place on a project**, such as the repository rule.
5. **[Nothing](#nothing).**

A ticket brings its own project. A project from a rule or a repository only counts when there is no ticket, or the ticket belongs to no project.

Within a step, organization rules run before personal ones. Rules for other tags all run; when two set the same tag name, the organization rule wins, then the one listed first. A tag set by hand or sent with the token entry wins over every rule.

## When prompts name several tickets {#several-keys}

By default the ticket named most often wins; on a tie, the one named last. Keys in the session title count as named before the prompts. An organization can switch this to an equal split between the tickets, or to the first one named.

<div class="note"><b>A mention is not work.</b> "The follow-up ticket will be KD-260" names a ticket the session did not work on. Rules cannot tell the difference. The <a href="/attribute/classifier">classifier</a> can, and you can always place a token entry by hand.</div>

## Summary match {#summary}

When no rule finds a ticket, Token Controller compares the token entry's summary with the titles of open tickets in the project the repository maps to, or else in the whole organization. If one ticket clearly fits, the token entry goes there, marked _matched by summary_ in the review list and in reports; otherwise the next step runs. An organization can switch summary matching off under _Rules_.

## When rules change {#changes}

A new or changed rule runs again over the token entries of every open period, so a fix also reaches past work. Token entries placed by hand keep their place. Closed periods never change; correct them with an [adjustment](/account/close#adjustments).

## When nothing matches {#nothing}

The token entry stays organization overhead with its person, and shows up in your [review list](/attribute/review), where you can add a ticket or project. See [cost kinds](/account/tags#kinds).

## Sessions and subagents {#sessions}

One session is one token entry, but cost is measured per message, so a session splits exactly. Telemetry and bills join token entries by message and request IDs too.

### Segments {#segments}

A session that changes branch or working directory is split there into segments. Each becomes a [split](/account/tags#splits) with its own tags, so one morning's session can land on two tickets. A session that returns to a branch adds to that branch's split.

### Subagents {#subagents}

A subagent is placed on its own signals when they find a place, such as a review subagent on a pull request. Otherwise it takes the tickets of the session that started it, and keeps the other tags its own signals give, such as `purpose: code-review`.
