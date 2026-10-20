---
title: "Tags, attributes and rules"
description: "Tag names and values with their defaults, attribute names and values, a person's attributes from a date, rules over signals, ticket systems, and the context the CLI reads."
---

Tags say what work was for; attributes say who the person is. Both are records with defaults: a feature brings its project, a project its client, a team its department. Rules turn signals into tags when a message arrives.

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
| `/ticket-systems` | Connect Jira, set the sync map, sync now. | Org manager, in the web app |

## Tag names and values {#tags}

```http
GET /api/v1/tags?as_of=2026-03-31
```

```json
[
  {
    "name": "feature",
    "built_in": true,
    "values": [
      {
        "id": "tag_5e0c…",
        "name": "feature",
        "key": "KD-1",
        "title": "Refund checkout for a returns portal",
        "description": "As a shopper I can return an order and get the refund to the same card.",
        "defaults": { "project": "shop" },
        "source_id": "10412",
        "source_status": "in progress",
        "recorded_at": "2026-02-20T15:30:00Z"
      }
    ]
  },
  { "name": "project", "built_in": true, "values": [ { "id": "tag_1a9b…", "name": "project", "key": "shop", "title": "Shop", "description": null, "defaults": { "client": "acme" }, "source_id": null, "source_status": null, "recorded_at": "2026-01-08T09:00:00Z" } ] },
  { "name": "client", "built_in": true, "values": [ { "id": "tag_77d1…", "name": "client", "key": "acme", "title": "Acme", "description": null, "defaults": {}, "source_id": null, "source_status": null, "recorded_at": "2026-01-08T08:58:00Z" } ] },
  { "name": "purpose", "built_in": false, "values": [] }
]
```

`project`, `feature` and `client` are built in. `source_id` and `source_status` are set by a ticket system's sync. A value's `id` is its event id; reversing it retires the value.

Add a tag name with `POST /tags` and `{"name": "purpose"}`: lower-case letters, digits, `-` and `_`, up to 40 characters. `201` when added, `200` when it is already on the list. Then add values:

```http
POST /api/v1/tags/project/values

{ "key": "shop", "title": "Shop", "defaults": { "client": "acme" } }
```

`201` with the value as above. A record with the same name and key supersedes the earlier one, so this is also how a project changes its client; the change applies to figures as of a later date, never to figures as of the cutoff. `dated` dates the record; before the [cutoff](/account/cutoff#adjust) that needs adjust. `404` for a tag name that does not exist. See [Tags](/account/tags).

## Attributes {#attributes}

`GET /attributes` has the same shape as `GET /tags`, for `team`, `department` and `location`, which are built in, and the names you add:

```json
[
  { "name": "team", "built_in": true, "values": [ { "id": "att_3c4d…", "name": "team", "key": "web", "title": "Web", "defaults": { "department": "engineering" }, "recorded_at": "2026-01-08T09:05:00Z" } ] },
  { "name": "department", "built_in": true, "values": [ { "id": "att_90ef…", "name": "department", "key": "engineering", "title": "Engineering", "defaults": {}, "recorded_at": "2026-01-08T09:04:00Z" } ] },
  { "name": "location", "built_in": true, "values": [] }
]
```

`POST /attributes/{name}/values` takes `key`, `title` and `defaults`, like a tag value. A person gets an attribute from a date:

```http
POST /api/v1/people/7f1c4e2a-…/attributes

{ "name": "team", "key": "web", "from": "2026-03-01" }
```

`201` with `id` (`pat_…`), `person`, `name`, `key` and `from`. The person keeps the earlier value for earlier days, so a team change on 1 April moves April's cost and leaves March's where it was. Left out, `from` means every day. The `{id}` is the member's id from [`GET /members`](/reference/api/organization#members). See [Attributes](/account/attributes).

## Rules {#rules}

A rule is a pattern over one signal that gives a tag value. It runs when a message arrives and back over unassigned messages since the cutoff.

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
| `name` | The name people see. |
| `signal` | `branch`, `working_directory`, `repository`, `pull_request_title`, `subagent_type`, `prompt_keys`, `agent_name`, `config:<name>` or `resource:<name>`. |
| `pattern` | A regular expression. A group named `key` picks the tag value's key; without one, the whole match is the key. |
| `tag_name` | The tag name the rule sets, such as `feature` or `project`. |
| `key` | A fixed key, such as `shop`, for "repository `kestrel/shop` is project Shop". Left out, the rule sets the key the pattern finds. |
| `order` | Lower runs first. Default: after the rules there are. |

`201`:

```json
{
  "id": "rul_6a1f…",
  "scope": "organization",
  "order": 1,
  "name": "feature key in branch",
  "signal": "branch",
  "pattern": "(?<key>KD-\\d+)",
  "tag_name": "feature",
  "key": null,
  "recorded_at": "2026-01-08T09:10:00Z"
}
```

`GET /rules` lists the organization's rules and your own in the order they run; `scope` is `organization` or your email. A rule is removed by reversing its `id`. `PUT /me/rules` takes a list of rules in the same shape and replaces your own; they run after the organization's, on your messages only. See [Rules](/attribute/rules).

## The CLI's context {#context}

`GET /me/context` is what `tc` reads once to split sessions on the machine the way the service does:

```json
{
  "as_of": "2026-03-10T07:00:00Z",
  "organization": { "id": "…", "name": "Kestrel Digital", "slug": "kestrel" },
  "person": "mara@kestrel.example",
  "attributes": { "team": "web" },
  "reporting_currency": "USD",
  "cutoff": "2026-02-28",
  "names": [ { "name": "project", "kind": "tag", "built_in": true }, { "name": "team", "kind": "attribute", "built_in": true } ],
  "tag_values": [ … ],
  "attribute_values": [ … ],
  "rules": [ … ],
  "price_table": [ … ]
}
```

`tag_values` are the values you may view, `rules` the organization's and your own in order, `price_table` the [rows in force](/reference/api/organization#price-table).

## Ticket systems {#ticket-systems}

An org manager connects Jira Cloud in the web app; its issues become features with their project as default, synced every 15 minutes. The token is never sent back.

| Endpoint | What it does |
| --- | --- |
| `GET /ticket-systems` | The connected systems, each with `id`, `kind`, `site`, `key_hint`, `status` (`ok` or `failing`), `message`, `last_sync_at` and `mappings`. |
| `POST /ticket-systems` | `{"kind": "jira", "site": "https://acme.atlassian.net", "email": "…", "token": "…"}`. The first sync starts right away. `400` when the address or the token is not usable. |
| `GET /ticket-systems/{id}/projects` | The projects the system shows the token, as `key` and `name`, to map. |
| `PUT /ticket-systems/{id}/mappings` | The sync map, replacing the earlier one: a list of `{"external": "KD", "project": "shop", "fields": ["title", "description", "type", "status", "parent", "assignee"]}`. `fields` left out copies all of them. `400` for an unknown project or one mapped twice. |
| `POST /ticket-systems/{id}/sync` | Sync now. `202`: it starts within a minute. |
| `DELETE /ticket-systems/{id}` | Disconnect. Its features stay and stop syncing. `204`. |

See [Jira](/account/tags#jira).
