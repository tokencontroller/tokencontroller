---
title: "Docker"
description: "Run the service and Postgres with Docker Compose."
---

Run the service and Postgres with Docker Compose.

## compose.yaml {#compose}

```yaml
services:
  tokencontroller:
    image: ghcr.io/tokencontroller/tokencontroller:1.0
    ports: ["8080:8080"]
    environment:
      DATABASE_URL: postgres://tc:secret@db/tc
      TC_PUBLIC_URL: https://tc.example.com
      TC_SECRET_KEY: change-me
      TC_SMTP_URL: smtp://user:pass@mail.example.com:587
    depends_on: [db]
  db:
    image: postgres:17
    environment:
      POSTGRES_USER: tc
      POSTGRES_PASSWORD: secret
    volumes: ["pgdata:/var/lib/postgresql/data"]
volumes:
  pgdata:
```

```sh
docker compose up -d
```

Put a reverse proxy with TLS in front of port 8080. The first person to sign in becomes the controller.
