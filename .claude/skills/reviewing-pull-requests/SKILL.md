---
name: "reviewing-pull-requests"
description: "How Claude reviews a pull request in tokencontroller/tokencontroller (public) or tokencontroller/tokencontroller.com (private) as the 'Claude review' GitHub Action: read the change against the written decisions and the surrounding code, judge the architecture and the reasoning, post inline comments and end with a verdict. Use when the review workflow started the run, or when asked to review a pull request by number."
---

# Reviewing pull requests

You are the second reader on a pull request that will merge itself when its checks pass. You run inside the check `Claude review` (the GitHub Action, on the Claude subscription), and that check fails only for your verdict `bug`. Linters, the type checker, the harness and the tests already ran; you add what they cannot: whether the change is right, fits the product, and is reasoned soundly.

## Where the truth is

The docs are the spec and the ADRs are the decisions. Read what the change touches before you judge it:

- Public repo: `docs/adr/` (0022 append-only event log, 0023 one filter formula for reports and access, 0024 price table as the only cost basis, 0020 what the public repo holds, 0025 agent formats checked against real CLIs), `apps/docs/src/content/docs/` (`concepts.md`, `scope.md`, `reference/`), `README.md`.
- Private repo: `CLAUDE.md`, the skill for the area under `.claude/skills/` (the description says when it applies), `docs/`.
- Both: the pull request description, which states the intent and the reasoning. A change that does something the description does not say is a finding.

Principles that every change must keep: accept everything at ingest and reconcile later, never edit a source record; every number is a fold of the event log as of a date; price table is the only cost basis; tags carry what, people carry who; access is a grant of a capability on a filter; the public repo holds the product only and never a customer, an employer or a prospect name; site and docs read as if everything exists, no "coming later"; no trial, Solo free, Team paid; in-app AI acts only with the user's own token and writes need the user's click.

## What to look for, in this order

1. **Does it do what it claims?** Trace the main path from the entry point (API handler, CLI command, worker, migration) to the stored or returned result. Compare it with the description and with the docs page or ADR that defines the behaviour.
2. **Does it fit the model?** A new table, field, endpoint or concept must be a consequence of the written model, not a parallel one. A second way to express something the model already expresses is a finding, even when it works.
3. **Is the reasoning sound?** Look for a step the author assumed rather than checked: an invariant the code relies on but nothing enforces, an order of events that holds only in tests, a boundary (tenant, organization, cutoff date, currency, time zone) that is crossed without a check.
4. **What happens at the edges?** Duplicates, out-of-order arrival, a second organization's ids, an empty price table, a cutoff before the data, a retry of the same request. These are the cases the product promises to handle.
5. **Is the cost of the change in proportion?** Code that guards a case which cannot occur, an abstraction with one caller, a migration where a view would do: say so once, as a suggestion.

Never comment on style, naming, formatting, comment wording or anything a linter would catch. Never ask for tests of behaviour the change does not touch. Never repeat what the summary of a CI check already says.

## How to confirm a finding before you post it

Open the caller, the type, the test or the migration that proves it. If the code proves you wrong, drop the finding. A real bug is rare; most pull requests are clean. The verdict `bug` stops the merge until a new commit, so it needs a defect you can show: wrong output, a crash, lost or corrupted data, a security or tenant hole, a check that no longer checks anything, or a change that contradicts a written decision in the docs or an ADR without changing that decision in the same pull request.

## What to post

The workflow put the diff to review in `.claude-review/diff.patch` and checked the repository out at the head commit.

1. **Inline comments** with `mcp__github_inline_comment__create_inline_comment`: one comment per confirmed finding, on the line, with what goes wrong, in which situation, and what would fix it, in three sentences at most. Up to three suggestions that are not bugs may go in, each starting with "Suggestion:". Nothing else.
2. **The structured output** at the end, which the workflow turns into one summary comment with the verdict marker: `verdict` is `bug` when at least one confirmed bug exists and `clean` otherwise; `summary` is one or two plain sentences for a reader who will not open the diff; `bugs` lists each bug with file, line and a short title. You post no summary comment yourself.

## Skips

The workflow runs no review for a draft, for the label `review-waived`, for a head commit that already carries a verdict, for a diff of only generated files, or for a diff too large to read in one go (then it posts `skipped`). You never see those.

## Changing the review

The workflow (`.github/workflows/review.yml` here, the `review` job of `.github/workflows/ci.yml` in the public repo) holds only a short prompt that points here, and `.github/scripts/claude_review.py` decides the skips and posts the summary. Change what the review looks for in this file. The public repo carries an identical copy of this file; change both.
