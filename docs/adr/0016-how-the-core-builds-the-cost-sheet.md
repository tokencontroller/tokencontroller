# How the core reconciles bills and builds the cost sheet

This fills in the Bills, Monthly close and Cost sheet pages and ADR 0004 where the code in `crates/core` needed details.

- **Token entries are reconciled at list value against the bill's gross.** Residual is gross minus the list value of the token entries charged on the bill, and can be negative. Discount and credit are their own lines, and net is gross minus discount minus credit; a bill whose amounts do not add up that way is refused. Billed values are kept on token entries but do not change the check.
- **The caller decides which bill charged which token entry.** A reseller such as Cursor charges for models of other providers, so the model's provider cannot decide it. The service groups splits by bill; the core reconciles each group.
- **A token entry's cost on a model missing from the price table is not in its list value.** It shows in residual, and its line on the cost sheet is labelled unknown model.
- **Converting keeps totals.** A block's lines and residual are converted into the reporting currency together, with running totals, so they still add up to the converted gross. Each line is at most one nano-unit away from converting it alone.
- **Shown cents keep totals.** The cost sheet rounds a block's lines and residual to the minor unit with running totals, so the shown lines add up to the shown gross; net is shown gross minus shown discount and credit, and the total row adds up the shown figures. A line can be one cent away from rounding it alone.
- **The cost sheet CSV has one row per line:** `bill, line, cost_kind`, one column per rung of the ladder from the broadest, the extra tag columns, `amount` and `note`. Lines are `cost`, then `residual`, `gross`, `discount`, `credit` and `net` per bill, then `adjustment` or `late adjustment`, and one `total`.
- **Seat plans and subscriptions are not on the cost sheet.** As the Bills page says, their fees stay off the bill and off the cost sheet, and the usage inside them has no bill to add up to. That usage shows in reports, valued at API rates as estimates, and in the share view. The cost sheet has one block per bill, and its total is what bills charged plus adjustments, the money finance can check against invoices.
- **Reports count submitted cost at once, with approval on too**; the cost sheet counts approved cost only. The core gives both rules (`Status::counts_in_reports` and `Status::counts`).
- **With approval on, a rejected token entry also stops the close**, as it is not approved.
- **An adjustment carries tags, not only a cost object,** so its cost kind and ladder columns come from the same tag map as every other line.

## Why

The cost sheet is handed to finance, so every figure on it must add up as shown, in the reporting currency, without anyone fixing a cent by hand. Doing the rounding once, in the core, keeps the web app, the CSV export and the CLI from each rounding differently.
