---
title: "CSV import"
description: "Bring in usage no agent or tool reports, from a CSV file, through the CSV import endpoint."
---

You may have usage that no agent or tool reports, such as an export from another tool or figures someone kept in a spreadsheet. You can bring it in as a CSV file, one message or one session per row. The rows become source records like any other, dated when the work happened, and the messages are yours.

## CSV import {#csv}

```csv
agent_harness,provider,model,started_at,ended_at,tokens.input,tokens.output,tokens.cache_read,summary,feature,tags.purpose
claude-code,anthropic,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,41200,6100,18200,Fixed the cart total,KD-214,product
```

The columns are the fields of the [token entry schema](/reference/token-entry), with a dot for nested fields: `tokens.input`, `tokens.cache_write_1h` or any other `tokens.<kind>`, `signals.branch` or any other `signals.<name>`, and one `tags.<name>` column per tag, such as `tags.purpose`. `agent_harness`, `model`, `started_at` and `ended_at` are required. A token kind left out of a row has no tokens; a row with no counts at all is a count error and has no cost. Commas or semicolons both work, and up to 10,000 rows fit in one file. `tc export --csv` writes the same format.

Send the file signed in as yourself, first to `POST /api/v1/token-entries/csv/preview`, which shows what it read, with the problem in each row it could not read, and saves nothing. Then send it to `POST /api/v1/token-entries/csv`. If any row cannot be read, nothing is saved. The web app has no screen for this. The calls are on the [API page](/reference/api/messages#csv).

The file is stored as it arrived, as a received record, like everything a usage endpoint takes in; see [What the usage endpoints keep](/collect#received).

## One message at a time {#manual}

For a single message, send it as a [token entry](/collect/token-entries).
