---
title: "Token entry schema"
description: "The fields of a token entry, the API noun for one message or one session record, as the token entries endpoint and CSV import take them."
---

A token entry is one message, or one session with its messages, as the [token entries endpoint](/collect/token-entries) takes them, one entry or a list. [CSV import](/collect/manual#csv) takes the same fields as columns.

## Fields {#fields}

| Field | Meaning |
| --- | --- |
| `agent_harness` | The product the agent ran on, such as `claude-code` or `support-desk`. Required. |
| `agent_name` | A label for the agent, such as "review bot". Optional. |
| `provider` | Who charges for the model calls, spelled as bills spell it: `anthropic`, `openai`, `aws_bedrock`, `azure_foundry` or any other name in small letters, digits and `_`. Optional; send it when you know it. It is never guessed from the model's name, and the contract's provider wins. With the model and the contract it picks the [price table](/reference/pricing-tables) rows. |
| `model` | The model that did the work, such as `claude-sonnet-4-5`. Required. See [Model names](#model). |
| `started_at`, `ended_at` | When the session started and ended. Required. |
| `tokens` | Counts by kind: `input`, `output`, `cache_read`, `cache_write`, and any other kind by its name in small letters, digits and `_`, such as `cache_write_5m` or `cache_write_1h`. A kind left out has no tokens; no kind at all is a count error. A kind with no price row is listed and leaves the rest priced. See [token kinds](/reference/data-model#token-kinds). |
| `reported_cost` | The agent's own cost figure in US dollars, as a number or a decimal string such as `"0.031"`. Optional. Stored as a fact about the message; the cost shown is counts × the [price table](/account/price-table). |
| `summary` | What the session did, from one line to a short report. With the tag values, the only text stored while content text is off (see [Security and privacy](/security)). |
| `facts` | Session facts: `turns`, `tool_calls`, `retries`, `duration_seconds`, `subagents`, `files_touched`, `result` (a URL such as the pull request). All optional. Any other fact is kept as a signal. |
| `feature`, `project` | Where it belongs: a feature key such as `KD-214`, a project by key. Optional. A tag named here wins over every rule; without one, [rules split it](/attribute/rules#order). |
| `tags` | Any other tags, such as `{"purpose": "code-review"}`. One value per tag name. Optional. Tags sent here win over rules. |
| `signals` | Names and values that may help to assign the session, which [rules](/attribute/rules) read. Optional and open. See [Signals](#signals). |
| `session_id` | The agent harness's id for the session. Required for a session record. A message seen again with the same session id and message id is the same record. |
| `messages` | The session's model calls. Optional for a session record; with them, a session that telemetry also reported is counted exactly once, call by call. See [Messages](#messages). |
| `splits` | How the session is split. Optional; without it the session is one split, or rules split it. See [Splits](#splits). |
| `local_id` | The sender's own id for the entry, such as `claude-code:<session id>`. Optional. Sending it again adds nothing. |

Set by Token Controller: person, contract, cost, label, the defaults of the tags, the person's attributes. See [Which contract a message gets](/reference/data-model#contract).

## Signals {#signals}

These names have a field of their own. Any other name is kept as it came, such as `organization.id`, the organization an agent was signed in to, which the [shipped sign-in rule](/attribute/rules#defaults) reads.

| Signal | Meaning |
| --- | --- |
| `branch` | The git branch. |
| `working_directory` | The working directory. |
| `repository` | The repository, such as `github.com/kestrel/storefront`. |
| `pull_request`, `pull_request_title` | The pull request number and title. |
| `subagent_type` | The subagent type. |
| `keys` | Keys found in prompts, the session title and the pull request title. Only the keys, never the text. |
| `config` | Values from the repository's `.tokencontroller.toml`, as `name: value`. |

A signal named like a tag, such as `project` or `contract`, is your own assignment: it runs before the organization's rules, wins over them, and creates the tag value when it is new. An entry carries at most 200 signals besides the named ones, each up to 10,000 characters. Names that hold content text, such as prompts or tool output, are removed on arrival unless your organization switches keeping content text on; see [Security and privacy](/security).

## Model names {#model}

The model is taken as it is sent. When the entry names a provider, a prefix of that provider's name is dropped, so `anthropic/claude-sonnet-4-5` with `"provider": "anthropic"` is priced as `claude-sonnet-4-5`. Without a provider, the provider is never read from the prefix: `anthropic/claude-sonnet-4-5` stays a model name of its own, and its messages are not priced yet. They are priced once a provider is sent, or once a [rule](/attribute/rules) or a hand assignment in the [review list](/attribute/review) puts them under a contract, whose provider then counts.

## Messages {#messages}

Each message is one model call. A session record's `tokens` are the sum of its messages'.

| Field | Meaning |
| --- | --- |
| `message_id` | The agent harness's id for the call, unique within the session. Required. For Claude Code it is the API request id (`req_…`), the id its telemetry sends for the same call. |
| `at` | When the call was made. Required. |
| `tokens` | Counts by kind, as above. Required. |
| `model` | The model of this call, when it differs from the session's. Optional. |
| `reported_cost` | The agent's own cost figure for this call, in US dollars. Optional; a fact, never the figure. |

## Splits {#splits}

Each split has its own `tags` and a `fraction`; the fractions add up to 1. A split on a list of `message_ids` instead of a fraction covers exactly those messages.
