---
title: "Price tables"
description: "Published prices per model, versioned and kept with each closed period."
---

Price tables hold the published price per model. They turn token counts into list value when an agent does not report its own cost.

## Where prices come from {#source}

Token Controller refreshes prices daily from LiteLLM and models.dev; if one source fails, the refresh carries on with the other. Every change makes a new version. A self-hosted instance without internet access switches the refresh off with [`TC_PRICES_REFRESH`](/self-hosting/configuration#vars).

## Prices and closed periods {#periods}

A period keeps the price table it was closed with, so a closed cost sheet never changes when prices do.

## Unknown models {#unknown}

A model missing from the price table is labelled unknown model. Its cost is never shown as zero.
