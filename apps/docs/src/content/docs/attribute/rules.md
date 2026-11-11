---
title: "Rules"
description: "Signals against tags, how rules turn a pattern over any signal into a tag value or a contract, the shipped rules, scope and order, running back in time, remembering a hand assignment, and splits."
---

A rule is a pattern over signals that gives a tag value, such as "key `^KD-\d+` in the branch is that feature" or "branch starts with `review/` is `purpose: code-review`". Set rules once, and people only fix the exceptions.

## Signals are not tags {#signals}

A signal is a name and a value that came with a message and may hint at where its cost belongs. A tag is a decision. Rules make the step from one to the other.

| Signal | Comes from |
| --- | --- |
| Branch, working directory | Session files, through the mod and the CLI |
| Repository | Session files, telemetry, the GitHub Action |
| Pull request number and title | The mod, the CLI, the GitHub Action |
| Keys found in prompts and the session title | Read on the machine; only the keys leave it |
| Values in the repository's config file, `.tokencontroller.toml` | The mod, the CLI, the GitHub Action |
| Telemetry resource attributes | `OTEL_RESOURCE_ATTRIBUTES`; see [Telemetry](/collect/telemetry#tags) |
| Subagent type | Session files |
| Agent harness and agent name | Every message |
| Any other name a sender adds, such as `organization.id`, the organization an agent was signed in to | Token entries, telemetry, the mod |

The list is open, so a rule may read any signal by its name. A signal named like a tag, such as `project` or `contract`, is the sender's own assignment: it runs before every rule, wins over them, and creates the tag value when it is new.

Prompts and titles are read on the machine and only the keys are sent, unless your organization switches keeping content text on; see [Security and privacy](/security).

## What a rule is {#rule}

| Part | Meaning |
| --- | --- |
| Pattern | Any signal, a test (`is`, `starts-with`, `contains`, `matches`, `is-set`) and a value. A regular expression can capture a key, such as `^(KD-\d+)`. |
| Tag value | A fixed value, such as `project: shop` or `contract: claude-team`, or "that feature": the feature whose key the pattern captured. |
| Scope | The organization, or one person. Organization rules run before personal ones. |
| Order | Rules of one scope run in order. Rules resolve per tag name: for each name, the first rule that finds a value wins, so a rule on `feature` and a rule on `contract` both apply to one message. |

A rule writes a [split](#splits) on arrival. A new or changed rule also runs back over every message since the [cutoff](/account/cutoff) that has no hand assignment for its tag name, so a fix reaches past work. A project set by hand does not stop a contract rule. A rule that gives a contract may run back before the cutoff too, so a bill connected late can meet its messages. Figures as of the cutoff never change.

## The default rules {#defaults}

| Rule | Sets |
| --- | --- |
| Key matching a feature in the branch | That feature |
| Key matching a feature in the pull request title | That feature |
| Key matching a feature in the prompts or the session title | That feature |
| Repository that a rule maps to a project | That project, as project overhead |
| The Anthropic organization an agent was signed in to (`organization.id`) | The contract `anthropic-` followed by the first eight characters of the organization's id, created as a metered contract the first time the rule matches. It runs after your organization's own rules, so a rule of yours on `contract` wins. An API key names no organization, so those messages get their contract from the [other steps](/reference/data-model#contract). |

Add your own in the web app under _Rules_, in the mod's pane, or with `tc rules`. A rule can set any tag name.

## Order {#order}

For the feature and the project, the first step that finds a tag value wins:

1. **A tag set by hand.** Picked in the mod or the CLI, passed to the [GitHub Action](/collect/github-action), sent in the call, or sent as a signal named like the tag.
2. **Rules that find a feature**, organization rules first, then personal ones, each in its order.
3. **Rules that find a project**, such as the repository rule.
4. **[Nothing](#nothing).**

A feature brings its project as a default tag; a project from a rule only counts when there is no feature. For every other tag name, all rules run, and when two set the same name, the organization rule wins, then the one listed first. The contract follows its own steps and is set on the whole message before it is priced: by hand, by a rule, by the project's default, or because the provider has only one contract. See [Which contract a message gets](/reference/data-model#contract).

## When prompts name several features {#several-keys}

A session that names several features and that nobody decides is split equally between them. Assign it by hand to change that, and tick _remember_ to make the hand assignment a rule.

<div class="note"><b>A mention is not work.</b> "The follow-up will be KD-260" names a feature the session did not work on. Rules cannot tell the difference. Fix it in the <a href="/attribute/review">review list</a>; the fix wins over every rule.</div>

## Remember {#remember}

Every hand assignment in the review list, the mod or the web app offers _remember_. It creates a personal rule from the session's signals, such as "repository `kestrel/shop` is project Shop", and runs it back over your unassigned messages since the cutoff. Someone with assign on a filter can remember an organization rule.

## When nothing matches {#nothing}

A signal that matches no tag value leaves the message with no project: organization overhead, with its person. It is visible to whoever holds view there and shows in the [review list](/attribute/review), where one assignment can become a rule. A message that no step gives a contract waits there too.

## Splits {#splits}

A split is the assignment a rule or a person writes: on a session or on one message, a list of fractions and tag sets that add up to 1. Without one, a session is one split of 100 percent. A new split supersedes the earlier one on the same session or message; the earlier one stays in the log.

- **By message.** A session that changes branch or working directory gets a split per message, so one morning's session can land exactly on two features.
- **By fraction.** A person splits a session by hand, for example 60 percent on KD-1 and 40 percent on KD-2. These fractions are a judgment, not a measurement.
- **Equal.** A session that names several features and nobody decides is split equally.

A subagent is split on its own signals when they find a tag value, such as a review subagent on a pull request. Otherwise it takes the split of the session that started it, and keeps the other tags its own signals give, such as `purpose: code-review`.
