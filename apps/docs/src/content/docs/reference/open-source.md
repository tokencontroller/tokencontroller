---
title: "Open source and licenses"
description: "Licenses for the service and the local tools, how issues work, and the projects Token Controller builds on."
prev:
  link: "/plans"
  label: "Plans and limits"
next:
  link: "/self-hosting"
  label: "Overview"
---

Token Controller is open source. The service, the local tools and these docs live in one public GitHub repository. The Enterprise plan features are the one part that is not open source; see [Enterprise plan instances](/self-hosting/enterprise).

## Licenses {#licenses}

| Part | License |
| --- | --- |
| The service | AGPL-3.0. If you run a changed version for others, you share your changes. |
| CLI, Claude Code mod, GitHub Action, SDK | MIT. Use them in anything, including closed tools. |
| Enterprise plan features | Not open source. Licensed with the Enterprise plan. |

## Issues and pull requests {#contributing}

Issues are open: report bugs and ask questions on GitHub. Pull requests from outside are not accepted. The product has a fixed [scope](/scope), and one person answers for every line of it.

## Built on {#credits}

| Project | Used for |
| --- | --- |
| ccusage | How the CLI counts session files on the developer's machine: its readers follow ccusage's rules (MIT). |
| LiteLLM, models.dev | Model prices for the [price tables](/reference/pricing-tables), refreshed daily. The CLI carries a copy of LiteLLM's list for estimates on the machine. |
| Laya | The open local model in the reference [classifier](/attribute/classifier) script (Apache-2.0). |

## Where to next {#next}
