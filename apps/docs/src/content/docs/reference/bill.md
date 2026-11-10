---
title: "Bill schema"
description: "The fields of a bill, as the bills endpoint and CSV upload take them."
---

The fields of a bill, as the bills endpoint and [CSV upload](/account/bills#csv) take them. Amounts are decimal strings in `currency`.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `provider` | Who charged, such as `anthropic`, `openai` or `cursor`. Required. It is the contract's provider. |
| `contract` | The key of the contract the bill is for, such as `anthropic-api` or `claude-team`. Optional. A new key creates the contract. Left out, the bill goes to the provider's only contract, or creates one named after the provider when it has none; with two contracts of one provider, name it. |
| `contract_kind` | `metered` or `flat`, for a contract this bill creates. Optional; `metered` when left out. |
| `from`, `to` | The first and last day the bill covers, in UTC. Required. Reconciliation runs on this range. |
| `amount` | What the provider charged under the contract in the range. Required. For a flat contract, the fee plus extra usage. |
| `fee`, `extra_usage` | For a flat contract, the two parts of `amount`. Optional; with them the check shows what the plan covered. |
| `currency` | Such as `USD`. Required. |
| `exchange_rate` | To the organization's reporting currency, at the bill's last day. Optional; Token Controller fills it in when left out. |
| `lines` | The provider's own detail, such as cost per workspace and day. Optional. |
| `typed_in` | `true` when typed in by hand in the web app rather than sent by a program. Optional. |
| `final` | Set by Token Controller: whether the provider will no longer change the figures. A bill sent, uploaded or typed in is final. A pulled bill can come in before it is, such as [Anthropic Enterprise](/account/bills#anthropic-enterprise). |

A bill is an event. A restated bill is a new bill for the same contract and range; the earlier one is reversed and stays visible. Sending the same figures again adds nothing.
