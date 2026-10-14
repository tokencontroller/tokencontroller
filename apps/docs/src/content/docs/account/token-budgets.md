---
title: "Token budgets and estimates"
description: "Plan money for people, tickets, projects and periods, accept a token estimate, and compare both with actual cost."
---

A token budget is money a person has planned. A token estimate is what Token Controller expects a ticket to cost, before the work starts. Both are compared with actual cost, and neither ever stops spending.

![Token budgets: a monthly one for team web and the token estimates below](/screens/09-token-budgets.png)

## Token budgets {#budgets}

Put a token budget on any tag filter for a period: a person (for example 1,500 EUR a month), a ticket, a project, a team, or a set of tags such as `team: web` and `client: acme`. Type it, or accept a token estimate for a ticket. People managers set token budgets within the filters they are assigned. Token budgets can overlap, so two of them can count the same money; never add them up.

<span id="forecasts"></span>

## Token estimates {#estimates}

A token estimate is a suggestion from similar past work. Token Controller compares the new ticket's title and description with the summaries of past token entries, shows the tickets they belong to with a confidence, and proposes their median cost. You pick the matches that fit, or take the proposal as it is. It becomes a token budget only when a person accepts it.

The hosted service runs the comparison with a model on its own key. A self-hosted instance sets its own, or does without estimates.

Each closed period feeds the next estimates, so a new client project can be quoted from what similar work cost before.

## Token budget vs actual {#actual}

The _Token budget vs actual_ report shows each token budget, whatever filter it sits on, with its actual cost and the difference. Overrun is actual above the token budget; underrun is actual below. Each row opens down to tickets and token entries.

## Comparing {#compare}

Look at overruns and underruns alike. An overrun may be a problem. An underrun may mean someone can teach the others, or that the next token estimate should be lower.

## No caps {#no-caps}

A token budget never stops an agent and sends no alert. Your provider offers spend caps if you need them.
