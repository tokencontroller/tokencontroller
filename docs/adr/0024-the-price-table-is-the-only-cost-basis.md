# The price table is the only cost basis; reported cost is a fact, never a figure

The cost of a message is its token counts by kind (input, output, cache read, cache write) times the price table row for its provider, model and kind valid on its day. Nothing else produces a cost: an agent's own cost figure is stored on the message as a fact for comparison and is never shown or summed as the cost. A message without counts is a count error with no cost; a model without a row is labelled unknown with no cost; neither is ever zero, and both show in the reconciliation as cost the bill has that no message explains.

Price table rows are events (ADR 0022). On the hosted service a managed table is pulled in as events from our list; an organization adds rows and overrides, and the override wins. The open edition ships without prices and reads a CSV file the self-hoster brings. An Enterprise key includes the managed table, hosted by us or on the customer's servers. The managed table's source and its build live in the private repo; the open code only reads rows.

## Why

With two cost figures (reported and computed, "list value" and "billed value") every report had to say which one it showed, and the two never reconciled to the same bill. One basis makes the reconciliation a plain equation: bill = cost of messages + what no message explains, with the second part never negative. Counts are what every agent harness can report and what every provider bills; prices are what differs by contract, so that is the one thing an organization overrides. Keeping the managed table out of the open edition keeps the open code free of a list we maintain and lets an Enterprise key carry it.

## Consequences

- Every door requires the four counts; the telemetry receiver, the token entries endpoint, the CSV import and the CLI readers reject nothing but mark a message without counts as a count error.
- `reported_cost` stays on the message and in the raw export; no report sums it.
- The CLI carries a copy of the managed table for figures on the machine; the service's figures come from the organization's own rows.
