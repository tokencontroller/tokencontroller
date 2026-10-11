# Cost is stored per message; token entries and splits are built from messages

The unit the service stores is one model call: its tokens by kind, model, reported cost, timestamps, and the session and message IDs the agent harness gives it. A token entry is the sum of a session's messages; a segment is a run of messages on one branch and working directory; a split by segment is exact, and a split by hand is a share. Telemetry events and session-file entries for the same message are the same row, matched by session ID and message ID, so a session seen through both doors is counted once. Bills join on the same IDs where the provider gives them.

## Why

The docs promise that segments split exactly and that telemetry and token entries are joined and counted once. Both need the per-message grain; a per-session total cannot be split later or deduplicated against telemetry. The data stays small because no text is stored.
