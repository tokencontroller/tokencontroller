---
title: "Bill schema"
description: "The fields of a bill."
---

The fields of a bill, as the bills endpoint and CSV upload take them.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `period` | The calendar month in UTC, such as `2026-09`. |
| `provider` | Who charged, such as `anthropic`, `openai` or `cursor`. |
| `cost_source` | Usage at API rates, API key, cloud marketplace or reseller. Seats and subscriptions are flat fees and have no bill. |
| `gross`, `discount`, `credit`, `net` | The amounts. Discount and credit are positive and taken off gross, so net is gross minus discount minus credit. Gross may be left out; it is then net plus discount plus credit. |
| `currency` | Such as `USD`. |
| `exchange_rate` | To the organization's reporting currency, at the bill's date. Optional; Token Controller fills it in when left out. |
| `revision` | Set by Token Controller when a provider restates a bill. Sending the same figures again changes nothing. |
| `final` | Set by Token Controller: whether the provider will no longer change the figures. A bill sent, uploaded or typed in is final. A pulled bill can come in before it is, such as [Anthropic Enterprise](/integrations/anthropic-enterprise), and the close of its period waits for it. |
