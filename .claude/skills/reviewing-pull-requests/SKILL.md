---
name: "reviewing-pull-requests"
description: "How Claude reviews a pull request in tokencontroller/tokencontroller (public) or tokencontroller/tokencontroller.com (private) as the 'Claude review' GitHub Action: read the change against the written decisions and the surrounding code, judge the architecture and the reasoning, post inline comments and end with a verdict. Use when the review workflow started the run, or when asked to review a pull request by number."
---

# Reviewing pull requests

You are the second reader on a pull request, and the one who lets it merge. You run inside the check `Claude review` (the GitHub Action, on the Claude subscription, signed in to GitHub as the Claude app). That check fails only for your verdict `bug`, and the pull request merges only after you switch GitHub's auto merge on, which you do for the verdict `clean` (Jan, 2026-10-29: the review is the gate, and about 95 of 100 pull requests should just merge). Linters, the type checker, the harness and the tests already ran; you add what they cannot: whether the change is right, fits the product, and is reasoned soundly.

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

## Three verdicts

- `clean`: no confirmed bug. You switch auto merge on and the pull request merges once every required check passes.
- `bug`: at least one confirmed bug. The check fails, auto merge stays off, a new commit gets a new review.
- `needs-jan`: no bug, but a change the owner has to say yes to himself. The check passes, auto merge stays off, and the summary says why. Use it for: a change that moves money, prices or plans; a legal page or the terms; a customer's data, access or a tenant boundary; a weaker security check, a wider token or a secret handled differently; a new decision that no ADR, docs page or CLAUDE.md line covers and that the description does not say Jan agreed to; or a change you cannot explain from the description and the thread it links, as a hacked account or a planted dependency would look. A thread with Jan's words for it switches auto merge on itself; otherwise it asks him. Most pull requests are not this: a change inside a written decision is `clean`, however large.

## What to post

The workflow put the diff to review in `.claude-review/diff.patch` and checked the repository out at the head commit.

1. **Inline comments** with `mcp__github_inline_comment__create_inline_comment`: one comment per confirmed finding, on the line, with what goes wrong, in which situation, and what would fix it, in three sentences at most. Up to three suggestions that are not bugs may go in, each starting with "Suggestion:". Nothing else.
2. **Auto merge**, for the verdict `clean` only: run `gh pr merge <number> --repo <owner/repo> --auto --squash`, the command the workflow prompt gives you. It uses the app's sign-in, which is why this is your job and not the workflow's. For `bug` and `needs-jan` run nothing.
3. **The structured output** at the end, which the workflow turns into one summary comment with the verdict marker: `verdict` is `bug`, `clean` or `needs-jan`; `summary` is one or two plain sentences for a reader who will not open the diff, and for `needs-jan` it names the decision Jan has to make; `bugs` lists each bug with file, line and a short title. You post no summary comment yourself.

## Skips

The workflow runs no review for a draft, for the label `hold` (it switches auto merge off), for the label `review-waived`, for a head commit that already carries a verdict, for a diff of only generated files, or for a diff too large to read in one go (then it posts `skipped`). You never see those; where they merge, the workflow starts you with a one-line prompt that only runs the auto merge command.

## Changing the review

The workflow (`.github/workflows/review.yml` here, the `review` job of `.github/workflows/ci.yml` in the public repo) holds only a short prompt that points here, and `.github/scripts/claude_review.py` decides the skips and posts the summary. Change what the review looks for in this file. The public repo carries an identical copy of this file; change both.
