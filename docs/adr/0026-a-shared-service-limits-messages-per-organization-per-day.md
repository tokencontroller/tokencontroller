# A shared service limits messages per organization per day

The usage endpoints take every message they can read (ADR 0018, ADR 0022): a message with an unknown model, an unknown tag or a late date is stored and counted, and reconciled later. One case is different. On a service that many organizations share, one organization that sends without end, by mistake or on purpose, fills the database every other organization reads from.

So the service has one plug-in point, `ServiceHooks::daily_message_limit`, that a build can use to set how many messages one organization may add per day, counted from 00:00 UTC. A batch that would pass the limit is refused whole with `429 too_many_requests` and a message that says the limit and when it starts again; nothing of it is stored, so the sender can send the same batch again the next day. A message already on record does not count again, also when a session record supersedes its telemetry event. Once the day is full, every batch is refused, so a full organization costs one count query, not a load of its log. The open build sets no limit, and neither does the Enterprise plan. The hosted service sets one per plan (docs: Plans and limits).

## Why

The limit protects the shared database, not the bill: it sits far above what a real organization of the plan's size sends in a day. It refuses a whole batch so that a refused batch never leaves half its messages behind. It counts message events, which are what the log grows by. It is a hook, not a setting, because only the service that hosts many organizations knows their plans.
