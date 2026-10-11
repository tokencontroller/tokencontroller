#!/bin/sh
# Serves the docs with live reload at http://localhost:8898.
cd "$(dirname "$0")" || exit 1
pnpm install --frozen-lockfile --filter @tokencontroller/docs... >/dev/null && exec pnpm dev
