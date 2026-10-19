---
title: "Open source and licenses"
description: "Licenses for the service and the local tools, how issues work, and the projects Token Controller builds on."
---

The service, the local tools and these docs live in one public GitHub repository. Enterprise features and our own billing and support tools are not open source; see [Enterprise plan instances](/self-hosting/enterprise#features).

## Licenses {#licenses}

| Part | License |
| --- | --- |
| The service and its web app | AGPL-3.0. If you run a changed version for others, you share your changes. |
| CLI, accounting core, Claude Code mod, GitHub Action, SDK | MIT. Use them in anything, including closed tools. |
| Enterprise plan features | Not open source. Licensed with the Enterprise plan. |
| Our billing and support tools | Not open source. They run only on the hosted service. |

## Issues and pull requests {#contributing}

Report bugs and ask questions in GitHub issues. Pull requests from outside are not accepted: the product has a fixed [scope](/scope), and one person answers for every line of it.

## Built on {#credits}

| Project | Used for |
| --- | --- |
| ccusage | How the CLI counts session files on the developer's machine: its readers follow ccusage's rules (MIT). |
| LiteLLM, models.dev | Sources of the managed [price table](/reference/pricing-tables). The CLI carries a copy for figures on the machine. |
