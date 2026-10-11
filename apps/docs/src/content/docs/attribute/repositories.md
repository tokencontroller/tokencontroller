---
title: "Repository mapping"
description: "Place sessions on a project by repository."
---

Mapping a repository to a project places every session in that repository on the project, when no ticket matches.

## Map a repository {#map}

In the web app, open the project and add its repositories, such as `kestrel/storefront`. One repository belongs to one project.

## What gets placed {#placing}

Sessions in a mapped repository with no ticket key land on the project as project overhead. Sessions outside any mapped repository and without a ticket key stay organization overhead, with their person.

## Tags from a config file {#config}

A repository can carry its own tags in `.tokencontroller.toml` at its root. Every session in it gets them, through the mod, the CLI or the GitHub Action.

```toml
[tags]
project = "storefront"
purpose = "product"
```

Only tag names on your organization's list are used. A ticket found by a rule still wins over the file.

Repository mapping is the only placing that works with [telemetry](/collect/telemetry) alone, because telemetry sees the repository but not the branch.
