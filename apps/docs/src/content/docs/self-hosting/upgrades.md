---
title: "Upgrades and backups"
description: "Upgrade by pulling a new image; back up Postgres."
---

Upgrading is pulling a new image. Backing up is backing up Postgres.

## Upgrade {#upgrade}

```sh
docker compose pull
docker compose up -d
```

The service updates the database when it starts. Read the [changelog](/changelog) before a new major version.

## Back up {#backup}

Back up the Postgres database with your usual tools, such as `pg_dump`. Nothing else holds state. The [raw export](/report/raw-export) is a second copy in plain JSON.
