# Sessions and subagents

From the docs pages [Sessions and subagents](https://docs.tokencontroller.com/attribute/rules#sessions) and [Rules](https://docs.tokencontroller.com/attribute/rules), and ADR 0006 and ADR 0015.

Kestrel Digital has a rule that tags a code review subagent's work `purpose: code-review`, and splits cost equally when prompts name several tickets. Claude Code reports no cost here, so every message is valued from its token counts.

**Mara's session** starts on the branch `KD-214-saved-card` and is measured message by message:

| Message | What | Placed |
| --- | --- | --- |
| m1, m2 | Work on the branch; telemetry carries no branch, so both stay on the session's. m2 is reported twice and counts once | KD-214 |
| m3 | An Explore subagent on the same branch | KD-214, by its own branch, joined with the session's split |
| m4 | A code review subagent in its own worktree on `review/KD-240` | KD-240, with `purpose: code-review` |
| m5 | Back on `main`; the prompt names KD-231 and KD-240 | half each |
| m6 | A code review subagent on `main` that names no ticket | the session's two tickets, half each, keeping `purpose: code-review` |
| m7 | Back on `KD-214-saved-card` | KD-214 again |

The session is 21.00 USD: 9.00 on KD-214, 9.00 on KD-240 (7.50 of it review) and 3.00 on KD-231 (1.50 of it review).

**Lena's session** stays on `main` but moves from the storefront working directory to the intranet one. Each segment is placed by its own repository: 3.00 on storefront, 6.00 on intranet.

The bill is 40.00 USD, so 10.00 is residual.

What it checks: segments by branch and working directory, joined duplicate reports, a subagent on its own signals and one following the session's tickets, the equal split of prompt keys, splits with equal tags joined, and a segment's own repository.
