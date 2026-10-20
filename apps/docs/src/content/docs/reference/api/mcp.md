---
title: "MCP server"
description: "The read-only Model Context Protocol server at /api/v1/mcp: six tools over tag values, attributes, people, plans and the cost report, with a read token."
---

`/api/v1/mcp` is a Model Context Protocol server, so an AI agent can read your organization in a conversation: which features exist, who is on which team, what was planned and what it cost. It only reads, and it sees what the token's creator sees.

```bash
claude mcp add --transport http token-controller \
  https://app.tokencontroller.com/api/v1/mcp \
  --header "Authorization: Bearer tc_read_…"
```

`POST /mcp` takes JSON-RPC messages over HTTP with a read token or a session; `202` for a notification, `401` without a token. The server is named `token-controller`.

## Tools {#tools}

| Tool | Returns | Arguments |
| --- | --- | --- |
| `organization` | The settings, cutoff, tag and attribute names, the price table in force and your own rules. | None |
| `tag_values` | Tag values you may see, with title, description, source id and defaults. | `name` such as `feature`; `query`, text in the key or title such as `KD-214`; `limit`, default 100, at most 500 |
| `attributes` | Attribute values such as teams and departments with their defaults. | None |
| `people` | Everyone with their role, attributes and a people manager's filters. | None |
| `plans` | The accepted estimates you may see, each with its cost and deviation, and the shares of people in projects. | None |
| `report` | C(F, w): the [cost report](/reference/api/reports#cost) inside your view grants. | `from`, `to` (required); `by`, `filter`, `as_of`, `currency`, `approved` as the report takes them |

Every tool is marked read-only. Nothing an agent says through it writes an event; a split, an estimate or a bill still goes through the [HTTP API](/reference/api) with a person's own token.
