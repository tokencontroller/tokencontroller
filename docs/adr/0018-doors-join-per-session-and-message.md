# Doors join per session and message; telemetry fills in until a session record claims the session

Builds on ADR 0006. Every door writes model calls into one messages table, unique per organization, session ID and message ID, so the same call seen twice is one row. Each session has at most one session record. Telemetry for a session nobody has sent a record for stands as the session on its own. When the CLI, the mod or the token entries endpoint sends a session record with the same session ID, it adds what telemetry cannot see: the summary, the session facts, the signals and the splits, and its messages join the telemetry rows by message ID. A session's token counts are always the sum of its counting messages.

Three rules keep a session counted once when the doors do not agree on IDs:

- A telemetry event without a message ID of its own gets one built from the session, time, model and tokens, prefixed `otel:`, so a retried export lands on the same row. These rows cannot match a session file, so once a session record with its own messages claims the session, they stop counting.
- A session record sent without messages counts as one message named after the session, until real messages for the session arrive; then that stand-in stops counting.
- Telemetry metrics are acknowledged and not counted. Agent harnesses send the same calls as log events or spans, one per call, and counting both would double the cost.

Resending a session record with the same session ID, or the same `local_id`, adds nothing new; it never adds a second one. Because every record is an event (ADR 0022), the later copy is kept in the log and only one copy counts.

## Why

The docs promise that telemetry and session files for one session are counted once, and that a submit adds the feature and the summary to counts telemetry already reported. Keeping the message as the only thing that is ever summed makes that true by construction, and the three rules cover the cases where one door knows less than the other.
