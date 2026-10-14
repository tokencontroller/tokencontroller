# A closed period keeps its prices

From the docs pages [Monthly close](https://docs.tokencontroller.com/account/close), [Adjustments](https://docs.tokencontroller.com/account/close#adjustments), [Price tables](https://docs.tokencontroller.com/reference/pricing-tables) and [Bills](https://docs.tokencontroller.com/account/bills).

Kestrel Digital closes October. Codex reports no cost of its own, so its token entries are valued with the price table, and October closed with version 1 of it. Version 2, a made-up price cut for `openai/gpt-5` in November, must not change October: entry 1 stays 7.50 USD instead of 6.00.

| # | What | Value |
| --- | --- | --- |
| 1 | Mara, Codex. Message m1 is reported twice, by telemetry and by the session file, and counts once | 7.50 |
| 2 | Mara, Claude Code, valued from its token counts | 10.50 |
| 3 | Tom, Codex, on a model the price table does not know | labelled unknown model, never zero in the bill: its cost is in residual |
| 4 | An automatic entry nobody has taken yet: no agent manager | 1.20, organization overhead |
| 5 | Lena's draft, never submitted | does not count |
| 6 | Lena's entry, rejected by her people manager | does not count |

There are two bills, each reconciled on its own: OpenAI 12.00 (residual 4.50) and Anthropic 14.00, restated once (residual 2.30).

After the close, the controller posts two adjustments: minus 2.25 for a test run entry 1 counted twice, and plus 6.30 for Tom's token sheet of 30 October, submitted after the close and so marked late. The total is 30.05.

The close check shows one entry without an agent manager and one rejected entry. With approval off, neither stops the close.

What it checks: price table versions, joining duplicate reports of a message, unknown models, two bills, adjustments and late adjustments, drafts and rejected entries left out, and the close check.
