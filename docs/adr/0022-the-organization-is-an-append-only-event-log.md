# The organization is an append-only event log; figures are folds as of a date

Everything an organization holds is an event in one ordered, append-only log: source records (messages, bills, estimates, tag values, attribute values, shares, price table rows), assignments (splits, accepts, grants), the cutoff, submits and approvals, rules, and reversals. No row is ever updated or deleted. A correction is a new event: a reversal cancels any earlier event, and a new assignment supersedes the earlier one on the same target. Every figure the service shows is a fold of the log up to a date, "as of": as of today for the live figures, as of the cutoff for the frozen ones, and every report carries that date.

One cutoff date per organization replaces calendar periods and the monthly close. Events dated before the cutoff need the adjust capability; rules run back only since it; figures as of it never change on their own. Restated bills, late messages and corrected splits are new events dated when they arrive, and an org manager with adjust may date them before the cutoff on purpose.

## Why

The old model locked closed months in the database, snapshotted price table and tag map versions, and posted signed adjustments into closed months. Each of those was a second mechanism for one need: a figure finance was given must not change. A fold as of a date gives that for free, for any date, and makes every other feature simpler: a restated bill is an event, a team change is an attribute value with a from date, a superseded plan stays readable as of the day it held. Reversal instead of edit means the log is the audit trail, and the raw export is the log itself.

## Consequences

- Tables are append-only; the service enforces it with database grants, not with triggers on "locked" rows.
- Every read query takes an as-of date and ignores events after it and events a reversal before it cancelled.
- Supersession is per target: the newest split on a session or message counts, the newest accept per tag set, the newest grant per person and capability.
- Nothing under `tests/golden/` holds a close; cases hold events and expected figures as of dates.
