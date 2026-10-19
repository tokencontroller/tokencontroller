---
title: "Bill schema"
description: "The fields of a bill, as the bills endpoint and CSV upload take them."
---

The fields of a bill, as the bills endpoint and [CSV upload](/account/bills#csv) take them. Amounts are decimal strings in `currency`.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `provider` | Who charged, such as `anthropic`, `openai` or `cursor`. Required. |
| `from`, `to` | The first and last day the bill covers, in UTC. Required. Reconciliation runs on this range. |
| `amount` | What the provider charged for metered usage in the range. Required. Seats and subscriptions are flat fees and have no bill. |
| `currency` | Such as `USD`. Required. |
| `exchange_rate` | To the organization's reporting currency, at the bill's last day. Optional; Token Controller fills it in when left out. |
| `lines` | The provider's own detail, such as cost per workspace and day. Optional. |
| `typed_in` | `true` when typed in by hand in the web app rather than sent by a program. Optional. |
| `final` | Set by Token Controller: whether the provider will no longer change the figures. A bill sent, uploaded or typed in is final. A pulled bill can come in before it is, such as [Anthropic Enterprise](/account/bills#anthropic-enterprise). |

A bill is an event. A restated bill is a new bill for the same provider and range; the earlier one is reversed and stays visible. Sending the same figures again adds nothing.
