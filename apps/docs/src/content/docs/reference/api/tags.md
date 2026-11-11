---
title: "Tags, attributes and rules"
description: "Tag names and values with their defaults, attribute names and values, a person's attributes from a date, rules over signals, Jira as a tag source, and the context the CLI reads."
---

Tags say what work was for; attributes say who the person is. Both are records with defaults: a feature brings its project, a project its client, a team its department. Rules turn signals into tags, including the contract, when a message arrives.

| Endpoint | What it does | Who |
| --- | --- | --- |
| `GET /tags` | Tag names with the values you may view. | Anyone |
| `POST /tags` | Add a tag name, such as `purpose`. | Org manager, in the web app |
| `DELETE /tags/{name}` | Remove a tag name you added. Built-in names stay; values on record stay in the log. | Org manager, in the web app |
| `POST /tags/{name}/values` | Add a tag value, or change one: the same key again supersedes the earlier record. | Assign on the value's tags with their defaults, in the web app |
| `GET /attributes` | Attribute names with their values. | Anyone |
| `POST /attributes` | Add an attribute name, such as `guild`. | Org manager, in the web app |
| `POST /attributes/{name}/values` | Add an attribute value, or change one. | Assign on the value with its defaults, in the web app |
| `POST /people/{id}/attributes` | Put an attribute on a person from a date. | Assign on the value with its defaults, in the web app |
| `GET /rules` | The organization's rules and your own, in the order they run. | Anyone |
| `POST /rules` | Add an organization rule. | Assign on the tag it sets, in the web app |
| `GET /me/rules`, `PUT /me/rules` | Your own rules; replace them. | Anyone, for themselves |
| `GET /me/context` | What a local tool needs to split sessions like the service does. | Anyone, for themselves |
| `/ticket-systems` | Connect Jira as a tag source, set the sync map, sync now. | Org manager, in the web app |

## Tag names and values {#tags}

```http
GET /api/v1/tags?as_of=2026-03-31
```

```json
[
  {
    "name": "client",
    "built_in": true,
    "values": [
      { "id": "tag_77d1…", "name": "client", "key": "acme", "title": "Acme", "description": null, "source_id": null, "source_status": null, "defaults": {}, "contract": null, "recorded_at": "2026-02-01T09:00:00Z" }
    ]
  },
  {
    "name": "feature",
    "built_in": true,
    "values": [
      {
        "id": "tag_5e0c…",
        "name": "feature",
        "key": "KD-1",
        "title": "Checkout redesign",
        "description": "A new checkout flow with saved addresses and one-page payment.",
        "source_id": "10412",
        "source_status": "in progress",
        "defaults": { "project": "shop" },
        "contract": null,
        "recorded_at": "2026-02-01T09:00:00Z"
      }
    ]
  },
  {
    "name": "project",
    "built_in": true,
    "values": [
      { "id": "tag_1a9b…", "name": "project", "key": "shop", "title": "Shop", "description": "Acme's web shop.", "source_id": null, "source_status": null, "defaults": { "client": "acme" }, "contract": null, "recorded_at": "2026-02-01T09:00:00Z" }
    ]
  },
  { "name": "purpose", "built_in": false, "values": [] }
]
```

Built-in names come first, then the names you added, each in alphabetical order. `project`, `feature` and `client` are built in. `contract` is built in too, for money; its values are listed with their terms by [`GET /contracts`](/reference/api/bills#contracts). `source_id` and `source_status` are set by a tag source's sync, such as the Jira issue id and its status. `contract` holds the terms on a value of `contract`, and is `null` on every other value. A value's `id` is its event id; reversing it retires the value.

Add a tag name with `POST /tags` and `{"name": "purpose"}`: lower-case letters, digits, `-` and `_`, up to 40 characters, and never `person`. `201` when added, `200` when it is already on the list. Then add values:

```http
POST /api/v1/tags/project/values

{ "key": "shop", "title": "Shop", "description": "Acme's web shop.", "defaults": { "client": "acme" } }
```

`201` with the value as above. Each default must be a value on record. A record with the same name and key supersedes the earlier one, so this is also how a project changes its client; the change applies to figures as of a later date, never to figures as of the cutoff. `dated` dates the record; before the [cutoff](/account/cutoff#adjust) that needs adjust. `contract` takes a contract's terms on a value of `contract` only; see [Contracts](/reference/api/bills#contracts). `404` for a tag name that does not exist. See [Tags](/account/tags).

## Attributes {#attributes}

`GET /attributes` has the same shape as `GET /tags`, for `team`, `department` and `location`, which are built in, and the names you add:

```json
[
  { "name": "department", "built_in": true, "values": [ { "id": "att_90ef…", "name": "department", "key": "engineering", "title": "Engineering", "defaults": {}, "recorded_at": "2026-02-01T09:00:00Z" } ] },
  { "name": "location", "built_in": true, "values": [] },
  { "name": "team", "built_in": true, "values": [ { "id": "att_3c4d…", "name": "team", "key": "web", "title": "Web", "defaults": { "department": "engineering" }, "recorded_at": "2026-02-01T09:00:00Z" } ] }
]
```

`POST /attributes/{name}/values` takes `key`, `title` and `defaults`, like a tag value. A person gets an attribute from a date:

```http
POST /api/v1/people/7f1c4e2a-…/attributes

{ "name": "team", "key": "web", "from": "2026-03-01" }
```

`201` with `id` (`pat_…`), `person`, `name`, `key` and `from`. The person keeps the earlier value for earlier days, so a team change on 1 April moves April's cost and leaves March's where it was. Left out, `from` means every day. The `{id}` is the member's id from [`GET /members`](/reference/api/organization#members). See [Attributes](/account/attributes).

## Rules {#rules}

A rule is a pattern over one signal that gives a tag value. It runs when a message arrives and back over messages since the cutoff that nobody assigned by hand. Rules resolve per tag name: the first matching rule for `feature` and the first for `contract` both apply. A rule on `contract` may run back before the cutoff, since figures as of the cutoff stay as they were.

```http
POST /api/v1/rules

{
  "name": "feature key in branch",
  "signal": "branch",
  "pattern": "(?<key>KD-\\d+)",
  "tag_name": "feature"
}
```

| Field | Meaning |
| --- | --- |
| `name` | The name people see, 1 to 100 characters. |
| `signal` | `branch`, `working_directory`, `repository`, `pull_request_title`, `subagent_type`, `prompt_keys`, `agent_name`, `config:<name>`, `resource:<name>` or any other signal by its name, such as `organization.id`. |
| `pattern` | A regular expression. A group named `key` picks the tag value's key; without one, the whole match is the key. |
| `tag_name` | The tag name the rule sets, such as `feature`, `project` or `contract`. |
| `key` | A fixed key, such as `shop` for "repository `kestrel/shop` is project Shop". It must be a value on record, or for `contract` a contract the organization has. Left out, the rule sets the key the pattern finds. |
| `order` | Lower runs first. Default: 10 more than the highest order there is, so the first rule is 10. |

`201`:

```json
{
  "id": "rul_6a1f…",
  "name": "feature key in branch",
  "scope": "organization",
  "order": 20,
  "signal": "branch",
  "pattern": "(?<key>KD-\\d+)",
  "tag_name": "feature",
  "key": null,
  "recorded_at": "2026-02-01T09:10:00Z"
}
```

The worked month has two rules on `contract` at order 10: `organization.id` matching `^kestrel-team$` gives `claude-team`, and `agent_name` matching `^review bot$` gives `anthropic-api`. Beside the organization's rules, Token Controller ships a sign-in rule that is not in this list: a message whose agent reports the Anthropic organization it was signed in to, as `organization.id`, gets a metered contract named after it, such as `anthropic-6f1e2a3b`, created the first time it matches. The organization's own rules on `contract` run before it.

`GET /rules` lists the organization's rules and your own in the order they run; `scope` is `organization` or your email. A rule is removed by reversing its `id`. `PUT /me/rules` takes a list of rules in the same shape and replaces your own; they run after the organization's, on your messages only. See [Rules](/attribute/rules).

## The CLI's context {#context}

`GET /me/context` is what `tc` reads once to split sessions on the machine the way the service does:

```json
{
  "organization": { "id": "…", "name": "Kestrel Digital", "slug": "kestrel" },
  "person": "mara@kestrel.example",
  "reporting_currency": "USD",
  "cutoff": "2026-02-28",
  "rules": [ … ],
  "names": [ { "name": "client", "kind": "tag", "built_in": true }, { "name": "team", "kind": "attribute", "built_in": true } ],
  "tag_values": [ … ],
  "attribute_values": [ … ],
  "attributes": { "team": "web" },
  "price_table": [ … ],
  "as_of": "2026-03-10T07:00:00Z"
}
```

`rules` are the organization's and your own in order, `names` every tag and attribute name, `tag_values` the values you may view, `attributes` yours today, `price_table` the [rows in force](/reference/api/organization#price-table).

## Jira {#ticket-systems}

An org manager connects Jira Cloud in the web app as a tag source; its issues become features with their project as default, synced every 15 minutes. The paths keep the name `ticket-systems`. The token is never sent back.

| Endpoint | What it does |
| --- | --- |
| `GET /ticket-systems` | The connections, each with `id`, `kind`, `site`, `key_hint`, `mappings`, `last_sync_at`, `status` (`ok` or `failing`) and `message`. |
| `POST /ticket-systems` | `{"kind": "jira", "site": "https://acme.atlassian.net", "email": "…", "token": "…"}`. The first sync starts right away. `400` when the address or the token is not usable. |
| `GET /ticket-systems/{id}/projects` | The Jira projects the token can see, as `key` and `name`, to map. |
| `PUT /ticket-systems/{id}/mappings` | The sync map, replacing the earlier one: a list of `{"external": "KD", "project": "shop", "fields": ["title", "description", "type", "status", "parent", "assignee"]}`. `fields` left out copies all of them. `400` for an unknown project or one mapped twice. |
| `POST /ticket-systems/{id}/sync` | Sync now. `202`: it starts within a minute. |
| `DELETE /ticket-systems/{id}` | Disconnect. Its features stay and stop syncing. `204`. |

See [Jira](/account/tags#jira).
