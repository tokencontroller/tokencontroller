---
title: "CLI"
description: "Every tc command."
---

Every `tc` command.

## Commands {#commands}

| Command | What it does |
| --- | --- |
| `tc connect URL` | Connect to an organization with an invite link, or to a self-hosted instance. |
| `tc status` | Organization, person, agents found, rules, and today's cost and share on tickets. |
| `tc review` | List draft token entries and their tags. |
| `tc tag N NAME=VALUE…` | Set tags on token entry N, such as `ticket=KD-214` or `purpose=learning`. |
| `tc split N TICKET=SHARE…` | Split token entry N between tickets, such as `KD-231=60 KD-240=40`. |
| `tc submit` | Submit your token sheet. |
| `tc report --by TAG\|day` | Cost in a table, by any tag name or by day. `--filter team=web` limits it. |
| `tc export --csv` | All token entries as CSV. |
| `tc rules` | List, add and order personal rules. |
| `tc config KEY VALUE` | Settings, such as `classifier`. |

Every command takes `--help`.
