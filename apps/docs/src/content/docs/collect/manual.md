---
title: "Manual entry and CSV import"
description: "Add messages by hand or from a CSV file in the web app."
---

For usage that no agent or tool reports, add messages in the web app by hand or from a CSV file. Both are source records like any other, dated when they happen.

## Manual entry {#manual}

Under _Messages_, choose _Add_. Give the agent harness, model, time, the token counts by kind, a summary and its tags. Counts are required: without them the record is a count error and has no cost.

## CSV import {#csv}

Choose _Import CSV_ and upload a file with one message or one session per row. The columns are the fields of the [token entry schema](/reference/token-entry), with a dot for nested fields: `tokens.input`, `tokens.output`, `tokens.cache_write`, `tokens.cache_read`, `signals.branch`, and one `tags.<name>` column per tag, such as `tags.purpose`. `agent_harness`, `model`, `started_at`, `ended_at` and the four `tokens.*` columns are required. Commas or semicolons both work, and up to 10,000 rows fit in one file.

```csv
agent_harness,model,started_at,ended_at,tokens.input,tokens.output,tokens.cache_read,tokens.cache_write,summary,feature,tags.purpose
claude-code,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,41200,6100,18200,0,Fixed the cart total,KD-214,product
```

The import shows what it read, with the problem in each row it could not read, before anything is saved. The CLI's `tc export --csv` writes the same format.
