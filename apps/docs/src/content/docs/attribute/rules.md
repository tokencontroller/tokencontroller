---
title: "Rules"
description: "Signals against tags, how rules turn a pattern over signals into a tag value, their scope and order, running back since the cutoff, remembering a hand assignment, and splits."
---

A rule is a pattern over signals that gives a tag value, such as "key `^KD-\d+` in the branch is that feature" or "branch starts with `review/` is `purpose: code-review`". Set rules once, and people only fix the exceptions.

## Signals are not tags {#signals}

A signal is a fact about a session that hints at where its cost belongs. A tag is a decision. Rules make the step from one to the other.

| Signal | Comes from |
| --- | --- |
| Branch, working directory | Session files, through the mod and the CLI |
| Repository | Session files, telemetry, the GitHub Action |
| Pull request number and title | The mod, the CLI, the GitHub Action |
| Keys found in prompts and the session title | Read on the machine; only the keys leave it |
| Values in the repository's config file, `.tokencontroller.toml` | The mod, the CLI, the GitHub Action |
| Telemetry resource attributes | `OTEL_RESOURCE_ATTRIBUTES`; see [Telemetry](/collect/telemetry#tags) |
| Subagent type | Session files |

Prompts and titles never leave the machine; see [Security and privacy](/security).

## What a rule is {#rule}

| Part | Meaning |
| --- | --- |
| Pattern | A signal, a test (`is`, `starts-with`, `contains`, `matches`, `is-set`) and a value. A regular expression can capture a key, such as `^(KD-\d+)`. |
| Tag value | A fixed value, such as `project: shop`, or "that feature": the feature whose key the pattern captured. |
| Scope | The organization, or one person. Organization rules run before personal ones. |
| Order | Rules of one scope run in order; the first that finds a tag value for a tag name wins. |

A rule writes a [split](#splits) on arrival. A new or changed rule also runs back over every message since the [cutoff](/account/cutoff) that has no hand assignment, so a fix reaches past work. Figures as of the cutoff never change.

## The default rules {#defaults}

| Rule | Sets |
| --- | --- |
| Key matching a feature in the branch | That feature |
| Key matching a feature in the pull request title | That feature |
| Key matching a feature in the prompts or the session title | That feature |
| Repository that a rule maps to a project | That project, as project overhead |

Add your own in the web app under _Rules_, in the mod's pane, or with `tc rules`. A rule can set any tag name.

## Order {#order}

For the feature and the project, the first step that finds a tag value wins:

1. **A tag set by hand.** Picked in the mod or the CLI, passed to the [GitHub Action](/collect/github-action), or sent in the call.
2. **Rules that find a feature**, organization rules first, then personal ones, each in its order.
3. **Rules that find a project**, such as the repository rule.
4. **[Nothing](#nothing).**

A feature brings its project as a default tag; a project from a rule only counts when there is no feature. For every other tag name, all rules run, and when two set the same name, the organization rule wins, then the one listed first.

## When prompts name several features {#several-keys}

A session that names several features and that nobody decides is split equally between them. Assign it by hand to change that, and tick _remember_ to make the hand assignment a rule.

<div class="note"><b>A mention is not work.</b> "The follow-up will be KD-260" names a feature the session did not work on. Rules cannot tell the difference. Fix it in the <a href="/attribute/review">check list</a>; the fix wins over every rule.</div>

## Remember {#remember}

Every hand assignment in the check list, the mod or the web app offers _remember_. It creates a personal rule from the session's signals, such as "repository `kestrel/shop` is project Shop", and runs it back over your unassigned messages since the cutoff. Someone with assign on a filter can remember an organization rule.

## When nothing matches {#nothing}

A signal that matches no tag value leaves the message with no project: organization overhead, with its person. It is visible to whoever holds view there and shows in the [check list](/attribute/review), where one assignment can become a rule.

## Splits {#splits}

A split is the assignment a rule or a person writes: on a session or on one message, a list of fractions and tag sets that add up to 1. Without one, a session is one split of 100 percent. A new split supersedes the earlier one on the same session or message; the earlier one stays in the log.

- **By message.** A session that changes branch or working directory gets a split per message, so one morning's session can land exactly on two features.
- **By fraction.** A person splits a session by hand, for example 60 percent on KD-1 and 40 percent on KD-2. These fractions are a judgment, not a measurement.
- **Equal.** A session that names several features and nobody decides is split equally.

A subagent is split on its own signals when they find a tag value, such as a review subagent on a pull request. Otherwise it takes the split of the session that started it, and keeps the other tags its own signals give, such as `purpose: code-review`.
