# Token Controller mod for Claude Code

Records what Claude Code spends, message by message, and shows where it went without leaving the session. It runs in the terminal on macOS, Windows and Linux and in the Code tab of the Claude desktop app. In the VS Code panel, in CI and in cloud sessions it records messages and shows nothing.

```sh
# inside Claude Code
/plugin install token-controller --marketplace tokencontroller/tokencontroller
```

The mod keeps its messages through the CLI, `tc` ([install](https://docs.tokencontroller.com/collect/cli#install)). Connect it to your organization with `tc connect`, or leave it local. Until `tc` is installed, the mod keeps the session's summary and tags on the machine and the band says how to install it.

## What it does

- After every turn it runs `tc`, which reads the turn from Claude Code's own session files: every message with its token counts by kind (input, output, cache read, cache write) and model, the working directory, the branch, and feature keys in your prompts. Only the keys leave a prompt, never its text.
- During the session it writes the one-line summary with a small model call of its own and hands it to `tc record`, with the session facts it counts (turns, duration, subagent turns) and the feature and tags you set by hand.
- `tc` builds the session record and splits it by your rules. The mod does not split anything itself ([ADR 0010](../../docs/adr/0010-local-tools-share-the-cli.md)).
- A band above the prompt shows `$41.87 today · 91% on features · KD-214`, and a note when an org manager asks everyone to submit before the cutoff moves.
- A pane (`/tc`) shows the check list, where you fix a feature and submit; a report by feature, project, team or day; and your personal rules.
- `/tc feature KD-214`, `/tc tag purpose=spike`, `/tc report [by]` and `/tc submit` run at once, without a turn. Claude can call the mod's `report` tool to answer "what did KD-214 cost this week?".

Everything the docs promise is on the [Claude Code mod](https://docs.tokencontroller.com/collect/claude-code) page.

## Working on it

| Path | What it holds |
| --- | --- |
| `.claude-plugin/plugin.json` | The manifest. The marketplace that lists it is `.claude-plugin/marketplace.json` at the repo root. |
| `hooks/register.tsx` | The hooks: recording, the band, the pane, `/tc` and the `report` tool. |
| `hooks/lib/` | The plain logic: the record for `tc record`, reading `tc --json`, formatting, the summary prompt. |
| `types/index.d.ts` | The session values the band and the pane draw from. |
| `tests/` | Tests that run the hooks inside Claude Code's own engine against a `tc` answered from memory. |

```sh
pnpm lint && pnpm typecheck && pnpm test   # what CI runs: hooks/lib and its *.spec.ts files
claude plugin validate .                   # what the engine will load, and what it refuses
claude plugin test .                       # tests/*.test.ts inside the engine
claude --plugin-dir .                      # a session with this folder loaded, reloaded on save
```

CI has no Claude Code, so it checks `hooks/lib` only. Run `claude plugin validate` and `claude plugin test` before every change to `hooks/register.tsx`. MIT license.
