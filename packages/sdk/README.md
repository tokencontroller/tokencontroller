# @tokencontroller/sdk

Report what an agent spends to [Token Controller](https://tokencontroller.com) from any agent written in TypeScript or JavaScript. It sends one call per session to the [token entries endpoint](https://docs.tokencontroller.com/collect/token-entries); a token entry is one message, or one session with its messages.

```sh
npm install @tokencontroller/sdk
```

```ts
import { TokenController } from '@tokencontroller/sdk'

const tc = new TokenController({ token: process.env.TOKEN_CONTROLLER_TOKEN })

await tc.tokenEntries.create({
  agent_harness: 'support-desk',
  agent_name: 'email answers',
  model: 'claude-sonnet-5',
  started_at: '2026-09-18T09:12:00Z',
  ended_at: '2026-09-18T09:14:10Z',
  tokens: { input: 4120, output: 610, cache_read: 18200, cache_write: 0 },
  reported_cost: 0.031,
  summary: 'Answered refund question for order 1182',
  facts: { turns: 3, tool_calls: 2, duration_seconds: 130 },
  project: 'support',
  tags: { client: 'acme', purpose: 'support' },
  session_id: 'support-desk-1182',
})
```

- `token` is a service token (`tc_svc_…`), created in the web app under _Settings_. The person who creates it is the person of every message it reports.
- `url` points the client at a self-hosted instance. Without it the client talks to `https://app.tokencontroller.com`.
- Every field is described in the [token entry schema](https://docs.tokencontroller.com/reference/token-entry). `problems(entry)` lists what is wrong with an entry before you send it; `create` checks the same and throws a `TokenControllerError`.
- An entry with a `session_id` or a `local_id` is tried again after a network error, a 429 or a 5xx answer, because sending it again updates it instead of adding a second one. An entry without either is sent once.
- `tokens` carries all four kinds on every message: `input`, `output`, `cache_read` and `cache_write`. The cost shown is counts × the price table; `reported_cost` is kept as a fact. Money goes as a number or a decimal string such as `"0.031"`.
- An agent that knows its model calls sends them as `messages`, so a session that telemetry also reported is counted once, call by call. `splits` puts fractions of the session on different tag sets (`{ fraction: 0.6, tags: { feature: 'KD-1' } }`), or exact `message_ids`; without them, rules split the session. `facts` are the session facts: turns, tool calls, retries, duration, subagents, files touched, and the result, such as a pull request URL. `signals.keys` and `signals.config` are what rules read. Never send prompt text.

Messages sent this way count in reports on arrival, with the token's creator as their person.

Needs Node.js 22 or newer, or any runtime with `fetch`. No dependencies. MIT license.
