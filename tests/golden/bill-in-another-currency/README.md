# A bill in another currency

From the docs pages [Bills](https://docs.tokencontroller.com/account/bills) and [The cost sheet](https://docs.tokencontroller.com/account/close#cost-sheet), and ADR 0004 and ADR 0016.

Halden reports in euros. Its two bills are in US dollars, each with the exchange rate of its date.

**Anthropic, Enterprise plan, usage at API rates:** gross 1,000.00 USD, discount 100.00, credit 25.00, net 875.00, at 0.8613 EUR per USD. Four token entries explain 933.16 USD at list value; 66.84 is residual. The discount and credit are their own lines.

Converted one by one and rounded to the cent, the four lines and the residual would come to 861.31 EUR, one cent more than the converted gross of 861.30. The core converts and rounds with running totals instead, so the shown lines add up to the shown gross: the second line shows 287.09, where converting it alone would round to 287.10.

**OpenAI, API key:** gross 50.00 USD at 0.86. One Codex entry, valued from its token counts with the price table, explains 41.20 USD.

The total, 796.64 EUR, is the sum of the two converted nets.

What it checks: conversion at each bill's own rate, discount and credit, shown cents that add up, and each bill reconciled on its own.
