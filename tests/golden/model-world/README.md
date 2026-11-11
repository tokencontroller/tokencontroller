# The model world

The model world of ADR 0028: three contracts, four bills, two Codex sessions started in the same minute by one person, and every hard case of the data model, worked out by hand before the code was written. Prices per million: Claude Sonnet 4.5 at `anthropic` and at `aws_bedrock` alike, input 10, output 30, cache read 1, cache write 12.5, 1-hour cache write 20; GPT-5 at `openai` input 5, output 20, and at `azure_foundry` input 6, output 20. The contract `openai-api` has its own input price of 4. The cutoff is 1 March.

## The contracts

- `anthropic-api`, metered, created by the bills `b4` (February, recorded 10 March) and `b1` (March).
- `claude-team`, flat, fee 150 a month, created by the rule `team sign-in` the first time it matches `organization.id = org-team-1`.
- `openai-api`, metered, created by the bill `b3`. It is OpenAI's only contract, so OpenAI messages fall back to it.

## How each message gets its contract

| Message | Cost | Contract | Why |
| --- | --- | --- | --- |
| F1, F2 (February) | 100, 80 | `anthropic-api` by rule | The rule `api sign-in` arrives on 10 March, after the cutoff, and still runs back over them: contract assignment may run back before the cutoff. The feature rule of the same day does not, so F1 keeps no feature. As of the cutoff both have no contract. |
| S2 (two messages) | 350, 50 | `claude-team` by rule | Project `shop` is set by hand on the session; the contract rule still applies, since rules resolve per tag name. |
| S3 | 205.50 | `claude-team` by rule | 100 input + 90 output + 600k 5-minute writes at 12.5 (7.50) + 400k 1-hour writes at 20 (8). Its plain cache write of 1 million is the same tokens told coarser and is not added. The feature rule gives `KD-1`. |
| S4 | 300 | `anthropic-api` by project default | The sender's project `bot` brings `contract=anthropic-api`. |
| S5 | 100 | `anthropic-api` by sender | The sender sent `contract=anthropic-api`; it wins over the team sign-in rule. |
| S7 | 60 | `anthropic-api` by hand | Sent without a provider. Anthropic and Bedrock charge the same, so it is priced; the hand assignment makes its provider `anthropic`. |
| S8 | 70 | none | Split 50/50 by hand over `bot` and `shop`; the parts do not bring the same contract, and Anthropic has two contracts, so the fallback does not choose. |
| S9 | not priced yet | none | GPT-5 without a provider: OpenAI and Azure charge differently. Its own line with 1 million input tokens, never zero. |
| Codex C1, C2, C3 | 84 | `openai-api` by fallback | See below. |

## Codex counted once

Tom starts C1 at 09:00:05 and C2 at 09:00:35 on 10 March. Each call arrives twice: from telemetry with a built id (`otel:…`) and from the session file (`turn-N`). Matching is call by call inside each session, on model, counts and time: `otel:a` counts as `C1/turn-1`, `otel:b` as `C1/turn-2`, `otel:c` as `C2/turn-1`. `otel:a` and `C2/turn-1` have the same counts and are 28 seconds apart, so a match across sessions or on session totals would go wrong. `C1/otel:side` has no file message and keeps counting (2). C3 came from a script that sent telemetry under `C3-tel` and the file under `C3-file`: the telemetry session has no other source, so its call is matched in Tom's overlapping session. `C1/turn-2` carries 1 million `reasoning` tokens with no price: listed, the rest priced. Codex: 28 + 14 + 2 + 28 + 12 = 84.

## The checks

| Bill | Contract | Bill | Messages | Unexplained | Beside it |
| --- | --- | --- | --- | --- | --- |
| b1, March | `anthropic-api` metered | 485 | 300 + 100 + 60 = 460 | 25 | same provider no contract 70 (S8) |
| b2, March | `claude-team` flat | 190 = fee 150 + extra usage 40 | 605.50 | 0 | covered by the plan 605.50 − 40 = 565.50; same provider no contract 70 |
| b3, March | `openai-api` metered | 84 | 84 | 0 | contract from fallback 84 |
| b4, February | `anthropic-api` metered | 200 | 180 | 20 | |

March costs 1,219.50: by contract `anthropic-api` 460, `claude-team` 605.50, `openai-api` 84, none 70; by kind metered 544, flat (on a plan) 605.50, none 70; by project `shop` 740.50, `bot` 335, none 144.
