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

A token estimate is a probability distribution of what a piece of work will cost: the chance of each cost band in dollars, with its median. Token Controller's System 1 classifier makes it from the description of the work, trained on the text of all organizations: summaries, estimate descriptions and their tags. It answers with probabilities only, so no description, name or client of another organization can come back out. Accept the median, or set your own figure; it becomes a token budget only when a person accepts it.

An estimate needs no tag. Ask for one through the [Estimates API](/reference/api#estimates), keep it on its own, and tag it later with any tag, such as a project, an epic or a ticket. Token entries matched to it give its [delta](#delta).

The hosted service runs the classifier. A self-hosted instance points to a classifier of its own (see [Configuration](/self-hosting/configuration)).

Each closed period feeds the next estimates, so a new client project can be quoted from what similar work cost before.

## Delta {#delta}

Once the work is done, every estimate sits next to the actual cost of its token entries. The classifier reads the estimate's description and the entries' summaries and labels the delta: scope when more or less was built than described, trouble when agents looped, failed or redid work, or the estimate itself. Each delta trains the next estimate.

## Token budget vs actual {#actual}

The _Token budget vs actual_ report shows each token budget, whatever filter it sits on, with its actual cost and the difference. Overrun is actual above the token budget; underrun is actual below. Each row opens down to tickets and token entries.

## Comparing {#compare}

Look at overruns and underruns alike. An overrun may be a problem. An underrun may mean someone can teach the others, or that the next token estimate should be lower.

## No caps {#no-caps}

A token budget never stops an agent and sends no alert. Your provider offers spend caps if you need them.
