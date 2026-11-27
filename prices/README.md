# Prices

The service's built-in price table (ADR 0029, docs Reference > Price tables).

- `litellm.csv`: generated from LiteLLM's `model_prices_and_context_window.json` by `scripts/prices/update.py`. Do not edit by hand.
- `litellm-commit`: the LiteLLM commit it was read from.
- `overrides.csv`: our own rows. For its provider, model and kind, a row here replaces every LiteLLM row. Add a row here when LiteLLM lacks a model or has a price wrong.
- `LICENSE-litellm`: LiteLLM's MIT licence, which its price data comes under.

Refresh: `python3 scripts/prices/update.py`, then open a pull request with the diff.
