#!/usr/bin/env python3
"""Refreshes prices/litellm.csv from LiteLLM's price list (ADR 0029).

    python3 scripts/prices/update.py            # LiteLLM's main as of now
    python3 scripts/prices/update.py <commit>   # a given LiteLLM commit
    python3 scripts/prices/update.py --file model_prices.json --commit <sha>

Reads model_prices_and_context_window.json from github.com/BerriAI/litellm
at one commit, keeps the text models of the providers in PROVIDERS, and
writes their prices per million tokens into prices/litellm.csv. Rows are
records: a price seen before keeps its row, a changed price gets a new row
valid from today (UTC), and a model LiteLLM drops keeps its rows. The commit
read goes into prices/litellm-commit. prices/overrides.csv is ours and is
never touched here; its rows win over these (apps/server price_table).

Standard library only. Prints what changed.
"""

from __future__ import annotations

import csv
import datetime
import decimal
import json
import pathlib
import subprocess
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[2]
CSV_PATH = ROOT / "prices" / "litellm.csv"
COMMIT_PATH = ROOT / "prices" / "litellm-commit"
REPO = "https://github.com/BerriAI/litellm"
RAW = "https://raw.githubusercontent.com/BerriAI/litellm/{commit}/model_prices_and_context_window.json"

# LiteLLM's provider key -> ours. Explicit on purpose: the provider is never
# guessed (ADR 0028). A LiteLLM provider not listed here is left out.
PROVIDERS = {
    "anthropic": "anthropic",
    "openai": "openai",
    "gemini": "google",
    "vertex_ai": "gcp_vertex",
    "vertex_ai-anthropic_models": "gcp_vertex",
    "vertex_ai-language-models": "gcp_vertex",
    "vertex_ai-mistral_models": "gcp_vertex",
    "bedrock": "aws_bedrock",
    "bedrock_converse": "aws_bedrock",
    "azure": "azure_foundry",
    "azure_ai": "azure_foundry",
    "github_copilot": "github",
}

# The prefix LiteLLM puts in front of some model names, by its provider key.
PREFIXES = ("vertex_ai/", "gemini/", "azure/", "azure_ai/", "bedrock/", "github_copilot/", "anthropic/", "openai/")

# LiteLLM's field -> our token kind (ADR 0028: kinds are open names).
KINDS = {
    "input": "input_cost_per_token",
    "output": "output_cost_per_token",
    "cache_read": "cache_read_input_token_cost",
    "cache_write": "cache_creation_input_token_cost",
    "cache_write_1h": "cache_creation_input_token_cost_above_1hr",
}

MODES = {"chat", "responses", "completion"}

# The first day of a row for a model seen for the first time: prices stand
# from the start until a newer row (the old hand-kept table started here).
FIRST_DAY = "2025-01-01"

HEADER = ["provider", "model", "kind", "price_per_million", "valid_from"]


def per_million(cost: object) -> str | None:
    """A cost per token as a price per million, at most 9 decimals."""
    if isinstance(cost, bool) or not isinstance(cost, (int, float, str)):
        return None
    try:
        value = decimal.Decimal(str(cost)) * 1_000_000
    except decimal.InvalidOperation:
        return None
    if value <= 0:
        return None
    value = value.quantize(decimal.Decimal("0.000000001"), rounding=decimal.ROUND_HALF_UP).normalize()
    text = format(value, "f")
    return text


def model_name(key: str) -> str | None:
    """The model as a harness reports it, or None for names no harness sends."""
    for prefix in PREFIXES:
        if key.startswith(prefix):
            key = key[len(prefix):]
            break
    if "/" in key or key.startswith("ft:") or not key:
        return None
    return key


def convert(listing: dict) -> tuple[dict[tuple[str, str, str], str], list[str]]:
    """LiteLLM's list as {(provider, model, kind): price}, and the conflicts."""
    prices: dict[tuple[str, str, str], str] = {}
    seen: dict[tuple[str, str], str] = {}
    dropped: set[tuple[str, str]] = set()
    conflicts = []
    for key in sorted(listing):
        entry = listing[key]
        if not isinstance(entry, dict) or entry.get("mode") not in MODES:
            continue
        provider = PROVIDERS.get(entry.get("litellm_provider"))
        model = model_name(key)
        if provider is None or model is None:
            continue
        rows = {}
        for kind, field in KINDS.items():
            price = per_million(entry.get(field))
            if price is not None:
                rows[kind] = price
        if "input" not in rows or "output" not in rows:
            continue
        # A provider that lists no cache write price charges cache writes as
        # input: the tokens are billed as plain input.
        rows.setdefault("cache_write", rows["input"])
        name = (provider, model)
        if name in dropped:
            continue
        if name in seen:
            before = {k: v for (p, m, k), v in prices.items() if (p, m) == name}
            if before != rows:
                conflicts.append(f"{provider}/{model}: {seen[name]} and {key} differ; left out")
                for kind in before:
                    del prices[(provider, model, kind)]
                dropped.add(name)
            continue
        seen[name] = key
        for kind, price in rows.items():
            prices[(provider, model, kind)] = price
    return prices, conflicts


def read_rows() -> list[dict[str, str]]:
    if not CSV_PATH.exists():
        return []
    with CSV_PATH.open(newline="") as handle:
        return list(csv.DictReader(handle))


def merge(rows: list[dict[str, str]], prices: dict[tuple[str, str, str], str], today: str) -> tuple[list[dict[str, str]], list[str]]:
    latest: dict[tuple[str, str, str], dict[str, str]] = {}
    for row in rows:
        key = (row["provider"], row["model"], row["kind"])
        if key not in latest or row["valid_from"] >= latest[key]["valid_from"]:
            latest[key] = row
    changes = []
    out = list(rows)
    for key, price in sorted(prices.items()):
        before = latest.get(key)
        if before is None:
            out.append(dict(zip(HEADER, (*key, price, FIRST_DAY))))
            changes.append(f"new {'/'.join(key)} {price}")
        elif decimal.Decimal(before["price_per_million"]) != decimal.Decimal(price):
            if before["valid_from"] == today:
                before["price_per_million"] = price
            else:
                out.append(dict(zip(HEADER, (*key, price, today))))
            changes.append(f"changed {'/'.join(key)} {before['price_per_million']} -> {price} from {today}")
    out.sort(key=lambda r: (r["provider"], r["model"], r["kind"], r["valid_from"]))
    return out, changes


def main(argv: list[str]) -> int:
    commit = None
    path = None
    args = list(argv)
    while args:
        arg = args.pop(0)
        if arg == "--file":
            path = args.pop(0)
        elif arg == "--commit":
            commit = args.pop(0)
        else:
            commit = arg
    if commit is None:
        out = subprocess.run(["git", "ls-remote", REPO, "refs/heads/main"], capture_output=True, text=True, check=True)
        commit = out.stdout.split()[0]
    if path:
        listing = json.loads(pathlib.Path(path).read_text())
    else:
        with urllib.request.urlopen(RAW.format(commit=commit), timeout=60) as response:
            listing = json.load(response)
    prices, conflicts = convert(listing)
    today = datetime.datetime.now(datetime.UTC).date().isoformat()
    rows, changes = merge(read_rows(), prices, today)
    with CSV_PATH.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=HEADER, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    COMMIT_PATH.write_text(commit + "\n")
    for line in conflicts:
        print("conflict:", line)
    for line in changes:
        print(line)
    print(f"{len(rows)} rows from LiteLLM {commit[:12]}, {len(changes)} changed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
