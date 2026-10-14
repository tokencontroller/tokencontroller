---
title: "Bill schema"
description: "The fields of a bill, as the bills endpoint and CSV upload take them."
---

The fields of a bill, as the bills endpoint and [CSV upload](/account/bills#csv) take them. Amounts are decimal strings in `currency`.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `period` | The calendar month in UTC, such as `2026-09`. Required. |
| `provider` | Who charged, such as `anthropic`, `openai` or `cursor`. Required. |
| `cost_source` | `usage_at_api_rates`, `api_key`, `cloud_marketplace` or `reseller`. Required. Seats and subscriptions are flat fees and have no bill. When one provider has bills from several cost sources, its token entries are charged on the first in this order. |
| `gross`, `discount`, `credit`, `net` | The amounts. Discount and credit are positive and taken off gross, so net is gross minus discount minus credit. `net` is required. Gross may be left out; it is then net plus discount plus credit. |
| `currency` | Such as `USD`. Required. |
| `exchange_rate` | To the organization's reporting currency, at the bill's date. Optional; Token Controller fills it in when left out. |
| `lines` | The provider's own detail, such as cost per workspace and day. Optional. |
| `typed_in` | `true` when typed in by hand in the web app rather than sent by a program. Optional. |
| `revision` | Set by Token Controller when a provider restates a bill. Sending the same figures again changes nothing. |
| `final` | Set by Token Controller: whether the provider will no longer change the figures. A bill sent, uploaded or typed in is final. A pulled bill can come in before it is, such as [Anthropic Enterprise](/account/bills#anthropic-enterprise), and the close of its period waits for it. |
