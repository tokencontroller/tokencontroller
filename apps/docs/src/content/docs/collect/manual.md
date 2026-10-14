---
title: "Manual entry and CSV import"
description: "Add token entries by hand or from a CSV file in the web app."
---

For usage that no agent or tool reports, add token entries in the web app by hand or from a CSV file.

## Manual entry {#manual}

Under _Token entries_, choose _Add_. Give the agent harness, model, start and end time, tokens or cost, a summary and its tags. The token entry goes on your token sheet like any other.

## CSV import {#csv}

Choose _Import CSV_ and upload a file with one token entry per row. The columns are the fields of the [token entry schema](/reference/token-entry), with a dot for nested fields: `tokens.input`, `tokens.output`, `tokens.cache_write`, `tokens.cache_read`, `signals.branch`, and one `tags.<name>` column per tag, such as `tags.team`. `agent_harness`, `model`, `started_at` and `ended_at` are required. Commas or semicolons both work, and up to 10,000 rows fit in one file.

```csv
agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket,tags.team
claude-code,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,41200,6100,0.42,Fixed the cart total,KD-214,web
```

The import shows what it read, with the problem in each row it could not read, before anything is saved. The CLI's `tc export --csv` writes the same format.
