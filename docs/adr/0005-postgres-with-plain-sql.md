# Postgres only, plain SQL checked at compile time, no ORM

The service and its jobs talk to Postgres through `sqlx` with queries checked against the schema at compile time. There is no ORM and no query builder. Migrations are plain SQL files applied by the service at start. Postgres is the only supported database; the CLI uses SQLite for its local store and shares no SQL with the service.

## Why

Checked queries fail the build when the schema and the code drift, which matters when agents write most of the code. One database keeps the self-hosting docs to one line. An ORM would hide the reporting queries that are the product.
