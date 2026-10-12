# How the core places token entries and splits them

This fills in the Rules page, ADR 0006 and ADR 0008 where the code in `crates/core` needed details.

- **A placed ticket brings its project.** The project of a token entry with a ticket is the ticket's own project, or else the project whose ticket key pattern matches the key. A project named in the token entry, set in a repository's `.tokencontroller.toml` or found by the repository rule only counts when there is no ticket, or the ticket belongs to no project. So a project's cost is always the sum of its tickets.
- **Tags are layered, the upper winning.** Tags named by hand or sent with the token entry; then rules, the organization's before personal ones and the first listed winning; then the repository's config file and telemetry resource attributes; then tags inherited from the ticket, its parent tickets, the project and the person. `person` is always the agent manager. Tag names not on the organization's list are dropped and reported.
- **A leading `^` in a ticket key pattern means the key starts a word.** `^KD-\d+` finds `KD-214` in `feature/KD-214-fix`, but not in `XKD-214`. Keys are found in the branch, the pull request title, the session title and the prompts.
- **Keys named in the session title count as named before the prompts** when the organization picks the ticket named most often or first.
- **A subagent's calls are their own segment.** It is placed on its own signals; when they place it nowhere, it takes the placement of the session's segment on the same branch and working directory.
- **A message without a branch keeps the session's.** Telemetry carries no branch, so a message seen only through telemetry lands in the session's segment.
- **A token entry belongs to the period it started in, counted in UTC.** Periods are calendar months in UTC, the way providers count usage on their bills.
- **Splits with the same tags are one split.** A session that returns to a branch, or a subagent that ends up with the session's tags, adds to the split it matches.
- **Rule and summary-match steps never use a ticket the last sync could not find.** The organization keeps those keys, and rules and the summary match run again without them.

## Why

Placing must give the same answer on the machine and in the service (ADR 0010), so every choice the docs leave open is made once, here, and tested in `crates/core`. A ticket that could sit on a project other than its own would let a project's total differ from the sum of its tickets, which the cost sheet relies on.
