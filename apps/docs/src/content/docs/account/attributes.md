---
title: "Attributes"
description: "Attributes on people: team, department, location, their defaults, and how a message carries them."
---

An attribute is on a person, such as `team: web` or `department: engineering`. It says who the person is; a [tag](/account/tags) says what the work was for. The two words are different on purpose.

## Attribute values {#values}

Like tag values, attribute values are records with a key, an optional title and default attributes. Built in: `team`, `department`, `location`. An org manager adds names.

| Attribute | Example value | Default attributes |
| --- | --- | --- |
| `team` | `web` | `department: engineering` |
| `department` | `engineering` | None |
| `location` | `vienna` | None |

A person has at most one value per attribute name. Someone in two teams picks a main one, or the organization adds a second attribute name such as `guild`. There is no org chart: rollup comes from defaults, so cost per department is the sum of its teams without any tree.

## On messages {#messages}

A message carries its person's attributes as of its time. An attribute value on a person has a from date, so a team change on 1 April leaves March on the old team at every as-of date, and April on the new one.

## In filters {#filters}

Attributes and tags mix in one [filter](/account/tags#filters): cost of `team: web` on `client: acme`, the plan of `department: engineering`, a grant on `team: web`. A head of engineering is a people manager over `department: engineering`; a manager is never a tag or an attribute.

<div class="note"><b>Telemetry.</b> Agents that read <code>OTEL_RESOURCE_ATTRIBUTES</code> can send <code>team=web</code> with every message. That is a <a href="/attribute/rules#signals">signal</a>, not the attribute: a rule turns it into a tag value, and the person's attributes come from the person record.</div>
