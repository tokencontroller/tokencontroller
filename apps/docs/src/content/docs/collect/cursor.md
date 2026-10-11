---
title: "Cursor Enterprise"
description: "Report Cursor Enterprise usage by telemetry."
---

Cursor Enterprise pushes usage over OpenTelemetry to your organization's ingest URL, with its own names for the same fields. Token Controller maps them. Session files are not read.

## Set up {#setup}

Point the agent's OpenTelemetry exporter at `https://app.tokencontroller.com/api/v1/ingest/otlp` and send your organization token as `Authorization: Bearer tc_org_…`. Find the token under _Settings_ in the web app.

## What it sends {#sees}

Numbers and identifiers only: tokens, cost, model, person, repository, session and message IDs. See [Telemetry push](/collect/telemetry) for what telemetry can and cannot see.
