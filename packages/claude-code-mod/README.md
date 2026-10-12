# Token Controller mod for Claude Code

Records what Claude Code spends, turn by turn, and shows where it went without leaving the session. It runs in the terminal on macOS, Windows and Linux and in the Code tab of the Claude desktop app. In the VS Code panel, in CI and in cloud sessions it records and shows nothing.

```sh
# inside Claude Code
/plugin marketplace add tokencontroller/tokencontroller
/plugin install token-controller
```

The mod keeps its token entries through the CLI, `tc` ([install](https://docs.tokencontroller.com/collect/cli#install)). Connect it to your organization with `tc connect`, or leave it local. Until `tc` is installed, the mod keeps what it recorded on the machine and the band says how to install it.

## What it does

- After every turn it takes Claude Code's own usage (tokens by kind, the model, what the turn added to the session's cost), the working directory, the branch, the repository and ticket keys found in your prompts, and hands them to `tc record`. Only the keys leave a prompt, never its text.
- During the session it writes the one-line summary with a small model call of its own and hands that to `tc` too.
- `tc` builds the draft token entry and its segments, and places it with your rules. The mod does not place anything itself ([ADR 0010](../../docs/adr/0010-local-tools-share-the-cli.md)).
- A band above the prompt shows `$212 spent · 91% on tickets · KD-214`, and a note when your token sheet is due or late.
- A pane (`/tc`) shows the review list, where you fix a ticket and submit; a report by ticket, project, team or day; and your personal rules.
- `/tc ticket KD-214`, `/tc tag purpose=spike`, `/tc report [by]` and `/tc submit` run at once, without a turn. Claude can call the mod's `report` tool to answer "what did KD-214 cost this week?".

Everything the docs promise is on the [Claude Code mod](https://docs.tokencontroller.com/collect/claude-code) page.

## Working on it

| Path | What it holds |
| --- | --- |
| `.claude-plugin/plugin.json` | The manifest. The marketplace that lists it is `.claude-plugin/marketplace.json` at the repo root. |
| `hooks/register.tsx` | The hooks: recording, the band, the pane, `/tc` and the `report` tool. |
| `hooks/lib/` | The plain logic: records for `tc record`, reading `tc --json`, formatting, the summary prompt. |
| `types/index.d.ts` | The session values the band and the pane draw from. |
| `tests/` | Tests that run the hooks inside Claude Code's own engine against a `tc` answered from memory. |

```sh
pnpm lint && pnpm typecheck && pnpm test   # what CI runs: hooks/lib and its *.spec.ts files
claude plugin validate .                   # what the engine will load, and what it refuses
claude plugin test .                       # tests/*.test.ts inside the engine
claude --plugin-dir .                      # a session with this folder loaded, reloaded on save
```

CI has no Claude Code, so it checks `hooks/lib` only. Run `claude plugin validate` and `claude plugin test` before every change to `hooks/register.tsx`. MIT license.
