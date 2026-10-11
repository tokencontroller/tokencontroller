---
title: "Manual entry and CSV import"
description: "Add token entries by hand or from a CSV file in the web app."
---

The web app takes token entries by hand or from a CSV file, so Token Controller is useful even where no agent or local tool can report.

## Manual entry {#manual}

Under _Token entries_, choose _Add_. Give the agent harness, model, start and end time, tokens or cost, a summary and its tags. The token entry goes on your token sheet like any other.

## CSV import {#csv}

Choose _Import CSV_ and upload a file with one token entry per row. The columns are the fields of the [token entry schema](/reference/token-entry). The import shows what it read before anything is saved. The CLI's `tc export --csv` writes the same format.
