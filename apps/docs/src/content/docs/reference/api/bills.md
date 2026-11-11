---
title: "Bills"
description: "Send and read bills, upload a CSV bill or a cloud cost export, read the reconciliation of each bill with its contract, list and add contracts, and connect a provider's admin API so bills are pulled daily."
---

A bill is a provider's figure for one contract over a date range. The service checks each bill against the cost of the messages with that contract in its range; the rest is the line no message explains. Every bill endpoint is for org managers, except the list of contracts.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /bills` | Send a bill for a date range. | Org manager, signed in or with a service token |
| `GET /bills` | The bills in force, newest range first, each with its reconciliation. | Org manager |
| `GET /bills/{id}` | One bill and the bills it restated, oldest first. | Org manager |
| `POST /bills/csv/preview` | Read a CSV bill file and show the bills it holds, or the first problem. Saves nothing. | Org manager, signed in or with a service token |
| `POST /bills/csv` | Upload a CSV bill file. Every bill is checked before any is stored. | Org manager, signed in or with a service token |
| `GET /reconciliation` | Each bill against the cost of the messages with its contract in its range. | Org manager |
| `GET /contracts` | Every contract of the organization, from its bills, its values of `contract` and the rules that created one. | Anyone |
| `POST /tags/contract/values` | Add a contract with its provider, kind and fee, or change it. | Assign on it, in the web app |
| `GET /bill-connections`, `POST /bill-connections` | The connected bill sources; connect one with an admin key. | Org manager; connecting in the web app |
| `DELETE /bill-connections/{id}` | Disconnect a source. Its bills stay and stop updating. | Org manager, in the web app |
| `POST /bill-connections/{id}/pull` | Pull now instead of at the next daily pull. | Org manager, in the web app |

## Send a bill {#send}

The fields are the [bill schema](/reference/bill). This is the seat plan's bill of the [worked month](/concepts#month): a fee of 200 and 50 of extra usage.

```http
POST /api/v1/bills
Authorization: Bearer tc_svc_…

{
  "provider": "anthropic",
  "contract": "claude-team",
  "contract_kind": "flat",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "amount": "250.00",
  "fee": "200.00",
  "extra_usage": "50.00",
  "currency": "USD"
}
```

`contract` names the contract the bill is for; a key nobody has yet creates that contract, of the kind `contract_kind` names (`metered` when left out). Left out, the bill goes to its provider's only contract of that kind, else to a contract named like the provider, such as `anthropic`. `fee` and `extra_usage` are the parts of a flat contract's bill; each is at most `amount`.

`201 Created` when the figures differ from the bill in force for that provider, contract and range: a bill event is written and the earlier bill is reversed. `200 OK` with the unchanged bill when the same figures are already stored. The answer is the bill as `GET /bills` shows it:

```json
{
  "id": "bil_9d2f…",
  "provider": "anthropic",
  "contract": "claude-team",
  "contract_kind": "flat",
  "fee": "200.00",
  "extra_usage": "50.00",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "amount": "250.00",
  "currency": "USD",
  "exchange_rate": null,
  "exchange_rate_given": false,
  "source": "api",
  "final": true,
  "lines": [],
  "recorded_at": "2026-04-02T08:00:00Z",
  "recorded_by": "nadia@kestrel.example",
  "reversed_by": null,
  "reconciliation": {
    "currency": "USD",
    "contract": "claude-team",
    "contract_kind": "flat",
    "bill": "250.00",
    "messages": "400.00",
    "unexplained": "0.00",
    "fee": "200.00",
    "extra_usage": "50.00",
    "covered": "350.00",
    "same_provider_no_contract": "0.00",
    "contract_from_fallback": "0.00",
    "a_message_is_wrong": false,
    "count_errors": 0,
    "unknown_models": []
  }
}
```

`contract` is the contract the bill is for: the one it named, else the one it went to. `contract_kind`, `fee` and `extra_usage` are what the bill named, or `null`. `source` is `api`, `csv`, `manual` or `pulled`. `exchange_rate` is to the organization's reporting currency at the bill's last day: given with the bill, else filled in from the reference rates, and `null` when the bill is already in the reporting currency or no rate for that day is known yet. `final` says whether the provider will no longer change the figures: a bill sent, uploaded or typed in is final; a pulled bill can come in before it is. `reconciliation` is the check described [below](#reconciliation), or `null` while the bill's currency cannot be converted.

## Read bills {#read}

`GET /bills` takes `from` to keep bills whose range starts on or after that day, `to` for those that end on or before it, `provider` for one provider, `reversed=true` to include restated bills, `as_of` for the bills known at a moment, and `format=csv`. `GET /bills/{id}` returns a list: the bill and every bill it restated, oldest first, so a restatement can be followed.

## CSV upload {#csv}

Send the file as the body with `Content-Type: text/csv`. Without a query, the columns are those of the [bill schema](/reference/bill). With `cloud=bedrock`, `vertex` or `foundry` and `month=2026-03`, the file is that cloud's own cost export, summed into one bill for the month.

```json
{
  "bills": [
    { "provider": "aws_bedrock", "from": "2026-03-01", "to": "2026-03-31", "amount": "412.80", "currency": "USD", "exchange_rate": null, "lines": 188 }
  ],
  "problem": null
}
```

The preview shows the bills the file holds, with `lines`, how many lines of a cloud's export were summed. When something is wrong, `problem` has its `row` (the header is line 1), `column` and `message`, and `bills` is empty. `POST /bills/csv` answers `201` with `bills`, each as it now stands, and `changed`, how many became a new bill event; the rest were already stored with the same figures. A file with a problem saves nothing and gets `400`.

## Reconciliation {#reconciliation}

```http
GET /api/v1/reconciliation?as_of=2026-04-05
```

One row per bill in force, with the bill's `bill_id`, `provider`, `from`, `to` and `final`, and the check in US dollars. The worked month's three bills:

```json
[
  {
    "bill_id": "bil_4a71…", "provider": "anthropic", "from": "2026-03-01", "to": "2026-03-31", "final": true,
    "currency": "USD", "contract": "anthropic-api", "contract_kind": "metered",
    "bill": "270.00", "messages": "250.00", "unexplained": "20.00",
    "fee": null, "extra_usage": null, "covered": null,
    "same_provider_no_contract": "0.00", "contract_from_fallback": "0.00",
    "a_message_is_wrong": false, "count_errors": 0, "unknown_models": []
  },
  {
    "bill_id": "bil_b20c…", "provider": "openai", "from": "2026-03-01", "to": "2026-03-31", "final": true,
    "currency": "USD", "contract": "openai-api", "contract_kind": "metered",
    "bill": "300.00", "messages": "300.00", "unexplained": "0.00",
    "fee": null, "extra_usage": null, "covered": null,
    "same_provider_no_contract": "0.00", "contract_from_fallback": "300.00",
    "a_message_is_wrong": false, "count_errors": 0, "unknown_models": []
  },
  {
    "bill_id": "bil_9d2f…", "provider": "anthropic", "from": "2026-03-01", "to": "2026-03-31", "final": true,
    "currency": "USD", "contract": "claude-team", "contract_kind": "flat",
    "bill": "250.00", "messages": "400.00", "unexplained": "0.00",
    "fee": "200.00", "extra_usage": "50.00", "covered": "350.00",
    "same_provider_no_contract": "0.00", "contract_from_fallback": "0.00",
    "a_message_is_wrong": false, "count_errors": 0, "unknown_models": []
  }
]
```

For a metered contract, `bill` = `messages` + `unexplained`: 270 = 250 + 20. A negative `unexplained` sets `a_message_is_wrong`: a wrong count, a wrong model, a price row too high, or a message under the wrong contract. For a flat contract, `bill` = `fee` + `extra_usage` + `unexplained`, and `covered` = `messages` − `extra_usage` is what the plan was worth: 400 − 50 = 350. `fee` comes from the bill, or from the contract for a whole month; `extra_usage` from the bill, or as the bill less the fee; `covered` is `null` while the fee is not known. A flat check says a message is wrong when `covered` is below zero.

Beside every check: `same_provider_no_contract`, the cost of the messages of the bill's provider in its range with no contract, and `contract_from_fallback`, the part of `messages` whose contract came only from the provider having one contract. A check cannot look right by leaving messages out. `count_errors` are messages in the range with no counts; `unknown_models` are the models of messages not priced yet, as `provider/model`, or the model alone when the provider is not known. `provider=anthropic` keeps one provider's bills; `format=csv` gives the table as a file. See [Reconciliation](/account/bills#reconciliation).

## Contracts {#contracts}

```http
GET /api/v1/contracts?as_of=2026-04-05
```

```json
[
  { "key": "anthropic-api", "title": "Anthropic API", "provider": "anthropic", "kind": "metered", "fee": null },
  { "key": "claude-team", "title": "Claude Team plan", "provider": "anthropic", "kind": "flat", "fee": "200.00" },
  { "key": "openai-api", "title": null, "provider": "openai", "kind": "metered", "fee": null }
]
```

`kind` is `metered` (paid per token) or `flat` (a fee, such as a seat plan). `fee` is the fee per calendar month of a flat contract in US dollars, when known. `openai-api` came with its bill, so it has no title and nobody set it up. A shipped sign-in rule creates a contract too, such as `anthropic-6f1e2a3b`, the first time an agent reports the Anthropic organization it was signed in to.

To add a contract by hand, or to give a flat contract its title and monthly fee, add a value of the built-in tag name `contract`:

```http
POST /api/v1/tags/contract/values

{ "key": "claude-team", "title": "Claude Team plan", "contract": { "provider": "anthropic", "kind": "flat", "fee": "200" } }
```

`201` with the [tag value](/reference/api/tags#tags), whose `contract` holds the terms. A new contract needs `provider` and `kind`; only a flat contract has a fee. Sent again without `contract`, the terms stay as they were. A [price row](/reference/api/organization#price-table) may belong to one contract and wins there. A message gets its contract [by hand, from the sender, by a rule, from a project default or from the fallback](/reference/data-model#contract); set it by hand with [`POST /contract-assignments`](/reference/api/messages#contract-assignments).

## Bill connections {#connections}

An org manager connects a provider in the web app with an admin key, so bills are pulled every day. The key is never sent back; the list shows its last characters.

```http
POST /api/v1/bill-connections

{ "provider": "anthropic_console", "key": "sk-ant-admin…" }
```

`provider` is `anthropic_enterprise`, `anthropic_console` or `openai`; see [Connecting a provider](/account/bills#connect). The first pull starts right away and loads the current and the previous month. A pulled bill names no contract, so it goes to the provider's only metered contract, else to a contract named like the provider. `201` with the connection:

```json
{
  "id": "7c0e5b7e-…",
  "provider": "anthropic_console",
  "key_hint": "…4f2a",
  "last_pull_at": "2026-04-05T06:10:00Z",
  "status": "ok",
  "message": null
}
```

`status` is `ok` or `failing`, with `message` saying why the last pull failed. `POST /bill-connections/{id}/pull` answers `202`: the pull starts within a minute. `DELETE` answers `204`; the connection's bills stay and stop updating.
