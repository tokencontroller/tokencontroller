# Cost is stored per message; sessions and splits are built from messages

The unit the service stores is one model call, the message: its token counts by kind, model, provider, time, person, and the session and message ids the agent harness gives it. An agent's reported cost is stored beside them as a fact and is never the figure. A session is the sum of its messages; a split on a list of messages is exact, and a split by fraction is a judgment. Telemetry events and session-file entries for the same message are the same record, matched by session id and message id, so a message seen through both doors is counted once. Only messages are ever summed (ADR 0022).

## Why

The docs promise that a session that changes branch splits exactly by message and that telemetry and session files are counted once. Both need the per-message grain; a per-session total cannot be split later or deduplicated against telemetry. The data stays small because no text is stored.
