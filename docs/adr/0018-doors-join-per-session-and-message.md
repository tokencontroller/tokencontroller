# Doors join per session and message; telemetry fills in until a token entry claims the session

Builds on ADR 0006. Every door writes model calls into one messages table, unique per organization, session ID and message ID, so the same call seen twice is one row. Each session has at most one token entry. Telemetry for a session nobody has claimed builds an automatic token entry, submitted, as the docs promise. When a token sheet or the token entries endpoint sends a token entry with the same session ID, it takes over that token entry: its summary, ticket, tags and splits replace what telemetry gave, and its messages join the telemetry rows by message ID. A token entry's tokens and reported cost are always the sum of its counting messages.

Three rules keep a session counted once when the doors do not agree on IDs:

- A telemetry event without a message ID of its own gets one built from the session, time, model and tokens, prefixed `otel:`, so a retried export lands on the same row. These rows cannot match a session file, so once a token entry with its own messages claims the session, they stop counting.
- A token entry sent without messages counts as one message named after the token entry, until real messages for the session arrive; then that stand-in stops counting.
- Telemetry metrics are acknowledged and not counted. Agent harnesses send the same calls as log events or spans, one per call, and counting both would double the cost.

Resending a token entry with the same session ID, or the same `local_id`, updates it; it never adds a second one.

## Why

The docs promise that telemetry and token entries for one session are joined and counted once, and that a token sheet adds the ticket and summary to money telemetry already reported. Keeping the message as the only thing that is ever summed makes that true by construction, and the three rules cover the cases where one door knows less than the other.
