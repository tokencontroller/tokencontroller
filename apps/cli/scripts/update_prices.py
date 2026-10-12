#!/usr/bin/env python3
"""Refreshes apps/cli/prices.json from LiteLLM's model price list.

The CLI shows list value with no service connected, so it carries a snapshot of
the published prices of the models whose session files it reads (Claude and
OpenAI). The service keeps its own versioned price tables; this file is only
the CLI's offline estimate. Prices are written as US dollars per million tokens,
as decimal strings, so no float ever reaches the Rust code.

Usage: python3 apps/cli/scripts/update_prices.py [PATH_TO_LITELLM_JSON]
Without a path it downloads the list from GitHub.
"""

from __future__ import annotations

import json
import sys
import urllib.request
from decimal import Decimal
from pathlib import Path

SOURCE = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
OUT = Path(__file__).resolve().parent.parent / "prices.json"
PROVIDERS = {"anthropic", "openai"}
FIELDS = {
    "input": "input_cost_per_token",
    "output": "output_cost_per_token",
    "cache_write": "cache_creation_input_token_cost",
    "cache_write_1h": "cache_creation_input_token_cost_above_1hr",
    "cache_read": "cache_read_input_token_cost",
    "input_above_200k": "input_cost_per_token_above_200k_tokens",
    "output_above_200k": "output_cost_per_token_above_200k_tokens",
    "cache_write_above_200k": "cache_creation_input_token_cost_above_200k_tokens",
    "cache_read_above_200k": "cache_read_input_token_cost_above_200k_tokens",
}


def per_million(value: object) -> str:
    amount = Decimal(str(value)) * 1_000_000
    text = format(amount.normalize(), "f")
    return text


def main() -> int:
    if len(sys.argv) > 1:
        data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    else:
        with urllib.request.urlopen(SOURCE, timeout=30) as response:
            data = json.load(response)
    models = {}
    for name, entry in sorted(data.items()):
        if not isinstance(entry, dict) or entry.get("litellm_provider") not in PROVIDERS:
            continue
        if entry.get("mode") not in ("chat", "responses"):
            continue
        if "/" in name or name.startswith("ft:"):
            continue
        prices = {
            key: per_million(entry[field])
            for key, field in FIELDS.items()
            if isinstance(entry.get(field), (int, float))
        }
        if "input" in prices and "output" in prices:
            models[name] = prices
    OUT.write_text(
        json.dumps({"source": "LiteLLM", "unit": "USD per million tokens", "models": models}, indent=1)
        + "\n",
        encoding="utf-8",
    )
    print(f"{len(models)} models written to {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
