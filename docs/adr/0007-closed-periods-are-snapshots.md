# A closed period keeps its own price table and tag map

Price tables are versioned: every refresh that changes a price makes a new version, and nothing is overwritten. The tag map is versioned the same way. A closed period records the price table version and the tag map version it closed with, and every figure on its cost sheet is computed with those. Token entries, splits and tags in a closed period are locked at the database level, and the only way to change a closed period is a signed adjustment row.

## Why

The docs promise that a closed cost sheet never changes when prices change or when the tag map is edited. Snapshots by version are cheaper and clearer than copying every figure at close, and they let a report explain any number from the versions it was computed with.
