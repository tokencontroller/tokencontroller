---
title: "Bill schema"
description: "The fields of a bill."
---

The fields of a bill, as the bills endpoint and CSV upload take them.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `period` | The calendar month, such as `2026-09`. |
| `cost_source` | Subscription, seat, usage at API rates, API key, cloud marketplace or reseller. |
| `gross`, `discount`, `credit`, `net` | The amounts. |
| `currency` | Such as `USD`. |
| `exchange_rate` | To the organization's reporting currency, at the bill's date. Optional; Token Controller fills it in when left out. |
| `revision` | Set by Token Controller when a provider restates a bill. |
