# One service program, one Docker image, one Postgres database

The service is one Rust program in one Docker image. It serves the HTTP API, receives telemetry over OpenTelemetry (HTTP and protobuf), runs the background jobs (bill pulls, Jira sync, managed price table pulls, exchange rates, the request to submit) and serves the web app. All state lives in one Postgres database (15 or newer). The hosted Team plan, self-hosting and Enterprise plan instances run the same product. The hosted and Enterprise images are built on the open image and add their own parts through the plug-in points (ADR 0001, ADR 0020).

## Rules that follow

- Background jobs take a Postgres advisory lock before they run, so with several replicas each job still runs once, and a self-hosted single container needs no extra setup.
- The service applies database migrations when it starts. A new major version may change the API or the database; the changelog says so.
- Nothing else holds state: no Redis, no queue, no object store. A file that must be kept (a CSV import, a raw export) is built on request from the database.
- Required outside services are Postgres and SMTP only. The managed price table pull, the exchange rate refresh and the estimator call outside services and can each be switched off or replaced by a file, so an instance without internet access still works.

## Why

Self-hosters get one container and one database to run and back up. One program for every edition means hosted and self-hosted run the same product code, tested once here. The data is small (no transcripts), so one Postgres covers reporting too.
