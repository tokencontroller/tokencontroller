---
title: "Bills"
description: "Send and read bills, upload a CSV bill or a cloud cost export, read the reconciliation, and connect a provider's admin API so bills are pulled daily."
---

A bill is a provider's figure for one contract over a date range. The service checks each bill against the cost of the messages with that contract in its range; the rest is the line no message explains. Every bill endpoint is for org managers, except the list of contracts.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `POST /bills` | Send a bill for a date range. | Org manager, signed in or with a service token |
| `GET /bills` | The bills in force, newest range first, each with its reconciliation. | Org manager |
| `GET /bills/{id}` | One bill and the bills it restated, oldest first. | Org manager |
| `POST /bills/csv/preview` | Read a CSV bill file and show the bills it holds, or the first problem. Saves nothing. | Org manager |
| `POST /bills/csv` | Upload a CSV bill file. Every bill is checked before any is stored. | Org manager |
| `GET /reconciliation` | Each bill against the cost of the messages with its contract in its range. | Org manager |
| `GET /contracts` | Every contract of the organization, from its bills, its values of `contract` and the rules that created one. | Anyone |
| `POST /tags/contract/values` | Add a contract with its provider, kind and fee, or change its title. | Assign on it, in the web app |
| `GET /bill-connections`, `POST /bill-connections` | The connected bill sources; connect one with an admin key. | Org manager, in the web app |
| `DELETE /bill-connections/{id}` | Disconnect a source. Its bills stay and stop updating. | Org manager, in the web app |
| `POST /bill-connections/{id}/pull` | Pull now instead of at the next daily pull. | Org manager, in the web app |

## Send a bill {#send}

The fields are the [bill schema](/reference/bill).

```http
POST /api/v1/bills
Authorization: Bearer tc_svc_…

{
  "provider": "anthropic",
  "from": "2026-03-01",
  "to": "2026-03-31",
  "amount": "1000.00",
  "currency": "USD"
}
```

`201 Created` when the figures differ from the bill in force for that provider, contract and range: a bill event is written and the earlier bill is reversed. `200 OK` with the unchanged bill when the same figures are already stored. The answer is the bill as `GET /bills` shows it:

```json
{
  "id": "bil_9d2f…",
  "provider": "anthropic",
  "contract": "anthropic",
  "contract_kind": null,
  "fee": null,
  "extra_usage": null,
  "from": "2026-03-01",
  "to": "2026-03-31",
  "amount": "1000.00",
  "currency": "USD",
  "exchange_rate": "1",
  "exchange_rate_given": false,
  "final": true,
  "source": "api",
  "lines": [],
  "recorded_at": "2026-04-02T08:00:00Z",
  "recorded_by": "nadia@kestrel.example",
  "reversed_by": null,
  "reconciliation": {
    "currency": "USD",
    "bill": "1000.00",
    "messages": "950.00",
    "unexplained": "50.00",
    "a_message_is_wrong": false,
    "count_errors": 0,
    "unknown_models": []
  }
}
```

`contract` is the contract the bill is for: the one it named, else the one it went to. `contract_kind`, `fee` and `extra_usage` are what the bill named, or `null`. `source` is `api`, `csv`, `manual` or `pulled`. `exchange_rate` is to the organization's reporting currency at the bill's last day, filled in when left out, and `null` until a rate for that day is known. `final` says whether the provider will no longer change the figures: a bill sent, uploaded or typed in is final; a pulled bill can come in before it is.

## Read bills {#read}

`GET /bills` takes `from` and `to` to keep bills whose range lies inside, `provider` for one provider, `reversed=true` to include restated bills, `as_of` for the bills known at a moment, and `format=csv`. `GET /bills/{id}` returns a list: the bill and every bill it restated, oldest first, so a restatement can be followed.

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

The preview shows the bills the file holds, or `problem` with its `row`, `column` and a sentence; then `bills` is empty. `POST /bills/csv` answers `201` with `bills`, each as it now stands, and `changed`, how many became a new bill event; the rest were already stored with the same figures. A file with a problem saves nothing and gets `400`.

## Reconciliation {#reconciliation}

```http
GET /api/v1/reconciliation?provider=anthropic&as_of=2026-04-05
```

One row per bill in force, with the bill's `bill_id`, `provider`, `from`, `to` and `final`, and the check in US dollars:

```json
[
  {
    "bill_id": "bil_9d2f…",
    "provider": "anthropic",
    "from": "2026-03-01",
    "to": "2026-03-31",
    "final": true,
    "currency": "USD",
    "contract": "anthropic-api",
    "contract_kind": "metered",
    "bill": "270.00",
    "messages": "250.00",
    "unexplained": "20.00",
    "fee": null,
    "extra_usage": null,
    "covered": null,
    "same_provider_no_contract": "0.00",
    "contract_from_fallback": "0.00",
    "a_message_is_wrong": false,
    "count_errors": 0,
    "unknown_models": []
  }
]
```

For a metered contract, `bill` = `messages` + `unexplained`. A negative `unexplained` sets `a_message_is_wrong`: a wrong count, a wrong model, a price row too high, or a message under the wrong contract. For a flat contract, `bill` = `fee` + `extra_usage`, and `covered` = `messages` − `extra_usage` is what the plan was worth; `fee` comes from the bill, or from the contract for a whole month, and `covered` is `null` while it is not known. A flat check says a message is wrong when `covered` is below zero.

Beside every check: `same_provider_no_contract`, the messages of the bill's provider in its range with no contract, and `contract_from_fallback`, the part of `messages` whose contract came only from the provider having one contract. A check cannot look right by leaving messages out. `count_errors` are messages in the range with no counts; `unknown_models` are models with no price row, as `provider/model`. `format=csv` gives the table as a file. See [Reconciliation](/account/bills#reconciliation).

## Contracts {#contracts}

```http
GET /api/v1/contracts
```

```json
[
  { "key": "anthropic-api", "title": null, "provider": "anthropic", "kind": "metered", "fee": null },
  { "key": "claude-team", "title": "Claude Team plan", "provider": "anthropic", "kind": "flat", "fee": "200.00" }
]
```

A bill creates its contract when it names one nobody has yet, so most organizations never add one by hand. To add one, or to give a flat contract its monthly fee in US dollars:

```http
POST /api/v1/tags/contract/values

{ "key": "claude-team", "title": "Claude Team plan", "contract": { "provider": "anthropic", "kind": "flat", "fee": "200" } }
```

`kind` is `metered` or `flat`; only a flat contract has a fee. Sent again without `contract`, the terms stay as they were. A message gets its contract [by hand, a rule, a project default or the fallback](/reference/data-model#contract).

## Bill connections {#connections}

An org manager connects a provider in the web app with an admin key, so bills are pulled every day. The key is never sent back; the list shows its last characters.

```http
POST /api/v1/bill-connections

{ "provider": "anthropic_enterprise", "key": "sk-ant-admin…" }
```

`provider` is `anthropic_enterprise`, `anthropic_console` or `openai`; see [Connecting a provider](/account/bills#connect). The first pull starts right away and loads the current and the previous month. `201` with the connection:

```json
{
  "id": "7c0e5b7e-…",
  "provider": "anthropic_enterprise",
  "key_hint": "…4f2a",
  "status": "ok",
  "last_pull_at": "2026-04-05T06:10:00Z",
  "message": null
}
```

`status` is `ok` or `failing`, with `message` saying why the last pull failed. `POST /bill-connections/{id}/pull` answers `202`: the pull starts within a minute. `DELETE` answers `204`; the connection's bills stay and stop updating.
