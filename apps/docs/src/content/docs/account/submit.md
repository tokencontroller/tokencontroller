---
title: "Submit and approval"
description: "A person submits all their own messages inside a window, and an organization can require approval of each submission."
---

Submitting is how a person confirms which feature each of their sessions was for and what it did. Approval lets people managers accept what was submitted.

## Submit {#submit}

A submit covers all of a person's own messages inside a window: from their last submit to now. The person [checks](/attribute/review) their sessions, fixes the feature where a rule got it wrong, reads the summaries, and submits from the CLI, the Claude Code mod or the web app. Messages count in reports as they arrive; a submit marks them confirmed by their person. An empty submit says there was nothing to add.

Telemetry may already have reported the same messages; the submit adds the branch, the pull request and the summary, and the messages are the same records, counted once. Messages from a service token, such as CI, are never submitted by hand.

## Reminders {#reminders}

Before moving the cutoff, an org manager can ask everyone to submit, the way finance asks for timesheets before month end. The person sees the request in the mod and the web app and gets one email. Nobody is blocked and nothing is capped.

## Approval {#approval}

Each organization switches approval on or off.

| | Approval off | Approval on |
| --- | --- | --- |
| Submitted messages in reports | Count | Count, marked as not yet approved; reports can filter on that |
| Who approves | Nobody | Anyone with [approve](/account/roles#capabilities) on a filter the submission's messages are in, never their own |

A people manager approves a submission, or rejects it with a reason. A rejected submission goes back to its person's [check list](/attribute/review); they fix the splits and submit again. Approval is an event: a report as of an earlier date shows the state as it was then.
