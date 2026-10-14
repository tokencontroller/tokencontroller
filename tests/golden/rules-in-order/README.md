# Rules in order

From the docs pages [Rules](https://docs.tokencontroller.com/attribute/rules), [Ticket key patterns](https://docs.tokencontroller.com/attribute/ticket-keys) and [Projects and tickets](https://docs.tokencontroller.com/account/projects), and ADR 0015.

Kestrel Digital's storefront project finds keys with `^(KD|OPS)-[0-9]+`. Besides the default rules, the organization has three rules that set `purpose`, in this order: `review/` branches are code review, branches containing `spike` are research, and OPS tickets are operations. Mara has a personal rule: her `spike/` branches are learning. The epic KD-299 carries `feature: wishlist`; its story KD-300 sits inside it. The last ticket sync did not find KD-998.

| # | Signals | Ends up as | Why |
| --- | --- | --- | --- |
| 1 | branch `review/KD-231` | KD-231, code review | the key starts a word after `/` |
| 2 | branch `spike/KD-240-idea` | KD-240, research | the organization's rule wins over Mara's own |
| 3 | the same branch, `purpose=learning` set by hand | KD-240, learning | a tag set by hand wins over every rule |
| 4 | branch `review/spike-cache`, no key | storefront, code review | of two organization rules, the one listed first wins |
| 5 | prompts name KD-231 and KD-240 once each | KD-240 | a tie goes to the latest named |
| 6 | the session title names KD-240, a prompt KD-231 | KD-231 | the title counts as named before the prompts |
| 7 | branch `KD-300-share` | KD-300, wishlist | a story inherits its epic's tags |
| 8 | branch `OPS-12-restart` | OPS-12, operations | a rule can test the ticket |
| 9 | branch `KD-999-new` | KD-999, ticket not synced | a key that matches the pattern is used before its ticket syncs |
| 10 | branch `KD-998-gone` | storefront | the sync did not find KD-998, so the repository places it |
| 11 | `color=blue` set by hand | organization overhead | `color` is not on the organization's list, so it is dropped |

Entries cost 1.00 to 11.00 USD in order, 66.00 together, against a bill of 70.00.

What it checks: the order of named tags, organization rules and personal rules, multi-prefix patterns, the "most often" choice with ties, inheritance from parent tickets, rules on the ticket, unknown and missing tickets, and dropped tag names.
