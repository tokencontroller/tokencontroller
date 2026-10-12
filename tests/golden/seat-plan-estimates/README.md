# Seat plan estimates

From the docs pages [Bills](https://docs.tokencontroller.com/account/bills#what) and [Concepts](https://docs.tokencontroller.com/concepts), and ADR 0016.

Kestrel Digital uses Claude Code on a Claude Team plan. The seat is a flat fee: there is no bill for the usage inside it, so its token entries are valued at API rates as estimates. Felix also uses Codex on an OpenAI API key, which has a real bill.

| # | Token entry | Value |
| --- | --- | --- |
| 1 | Mara on KD-214, on the seat | 12.00, estimate |
| 2 | Tom on the storefront repository, on the seat | 4.50, estimate |
| 3 | Lena, no project, on the seat | 3.250005, estimate |
| 4 | Felix on KD-231, on the OpenAI API key | 8.00 |

Reports show all four, so each person's usage is visible: team web comes to 16.50. The cost sheet holds only the OpenAI bill, 10.00 with 2.00 residual, because seats stay off the bill and off the cost sheet.

What it checks: usage on a seat plan counts in reports but not on the cost sheet, and a token entry on no bill is still split and placed like any other.
