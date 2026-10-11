# Tags are stored on the split, copied at arrival, not joined at read time

Every tag a split has, including the ones inherited from its person, project or ticket, is stored as a row on the split: one row per tag name, with a unique constraint on (split, tag name). Inherited tags are copied when the token entry arrives. A later change to a person's or project's tags, or to a rule, reaches only open periods, by running the rules again and rewriting the rows of unlocked splits. Reports filter and group on these rows alone and never join back to the person or project.

## Why

The docs promise that history stays as it was and that cost grouped by one tag name adds up. Copied rows with one value per name give both by construction, and keep every report one query over one table. The cost of the copy is small: tags are short strings, and a token entry has a handful.
