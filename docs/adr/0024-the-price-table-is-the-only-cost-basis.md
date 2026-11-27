# The price table is the only cost basis; reported cost is a fact, never a figure

The cost of a message is the sum, over its token kinds that have a price, of its count times the price table row for its provider, model and kind valid on its day; a contract's own row wins over ours (ADR 0028). Nothing else produces a cost: an agent's own cost figure is stored on the message as a fact for comparison and is never shown or summed as the cost. A message without counts is a count error with no cost; a message without a price is "not priced yet" with its token counts; neither is ever zero, and both show in the check.

Price table rows are events (ADR 0022). The managed table is built into every edition from LiteLLM's price list with our own rows on top (ADR 0029); a self-hosted instance may add a CSV file of its own; an organization adds rows and overrides, and the override wins.

## Why

With two cost figures (reported and computed, "list value" and "billed value") every report had to say which one it showed, and the two never reconciled to the same bill. One basis makes the check a plain equation: bill of a contract = cost of its messages + unexplained, with the second part never negative (a flat contract adds its fee, ADR 0028). Counts are what every agent harness can report and what every provider bills; prices are what differs by contract, so that is the one thing a contract or an organization overrides.

## Consequences

- Token kinds are open names. The usage endpoints and the CLI readers reject nothing: a message without counts is a count error, and a kind with no price row is listed and leaves the rest priced.
- `reported_cost` stays on the message and in the raw export; no report sums it.
- The CLI carries no prices: offline it reads the person's own file, connected it uses the service's table.
