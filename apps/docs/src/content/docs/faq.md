---
title: "FAQ"
description: "Short answers to the questions people ask first about Token Controller."
prev:
  link: "/security"
  label: "Security and privacy"
next:
  link: "/glossary"
  label: "Glossary"
---

Short answers to the questions people ask first.

## Why can people managers see cost per person? {#per-person}

Because clients are billed for work per person, and the providers' own admin tools already show cost per person. Works councils sometimes ask for team totals only. Token Controller has no such mode, and a people manager sees only the people in their scope.

## Why are there no spend caps or spend alerts? {#caps}

Your provider already offers both. A token budget is a plan to compare against, and it never stops an agent. The only reminder Token Controller sends is for a late token sheet.

## Why doesn't the agent estimate its own cost? {#estimates}

Agent self-estimates match actual cost poorly, with a correlation of about 0.4. Token forecasts come from what similar work actually cost before.

## Do I need the hosted service? {#local}

No. The [CLI](/collect/cli) is fully useful on its own, including an export for a spreadsheet. Connect it to a service when you want to share numbers.

## Is it only for coding agents? {#agents}

No. Coding agents came first because their session files sit on the developer's machine. Any agent that can report home, such as a support agent, sends token entries through the [token entries endpoint](/collect/token-entries).

## Is there a mobile app? {#mobile}

No. The web app is readable on a phone. See [Scope](/scope).

## Can I invoice my clients from Token Controller? {#invoicing}

No. Token Controller ends at the [cost sheet](/report/cost-sheet). Pass-through to your clients happens in your own invoicing tool.

## Where to next {#next}
