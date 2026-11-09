# Database changes come in two releases

The service applies its migrations when it starts (ADR 0005). On a deploy the new version starts next to the one serving, runs its migrations and gets traffic only after its smoke test; the old version keeps serving until then and stays ready as the way back. So for a few minutes the version before runs on the new schema, and it must keep working there.

A migration therefore only adds: a table, a column that may be empty or has a default, an index. A change that takes something away or tightens it (dropping or renaming a table or column, changing a column's type, making a column required, adding a constraint to a table in use) is split over two releases. The first adds the new shape and stops using the old one. The second, once no released version uses the old shape, takes it away; it starts with a line `-- second step: <what no released version uses any more>`.

A test (`apps/server/tests/migrations.rs`) reads every migration from 9 November 2026 on and fails on a change of the second kind without that line.

## Why

A migration that breaks the version serving takes the service down for the minutes of the switch, and it breaks the way back: traffic sent back to the old version meets a schema it cannot read. Two releases cost one more pull request; a backward-compatible schema costs nothing at run time and needs no second database. The check is a test so it runs in the Rust job every pull request already waits for.
