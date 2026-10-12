# @tokencontroller/sdk

Report token entries to [Token Controller](https://tokencontroller.com) from any agent written in TypeScript or JavaScript. It sends one call per session to the [token entries endpoint](https://docs.tokencontroller.com/collect/token-entries).

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
  tokens: { input: 4120, output: 610, cache_read: 18200 },
  reported_cost: 0.031,
  summary: 'Answered refund question for order 1182',
  project: 'support',
  tags: { client: 'acme', purpose: 'support' },
  session_id: 'support-desk-1182',
})
```

- `token` is a service token (`tc_svc_…`), created in the web app under _Settings_. The person who creates it is the agent manager of everything it reports.
- `url` points the client at a self-hosted instance. Without it the client talks to `https://app.tokencontroller.com`.
- Every field is described in the [token entry schema](https://docs.tokencontroller.com/reference/token-entry). `problems(entry)` lists what is wrong with an entry before you send it; `create` checks the same and throws a `TokenControllerError`.
- An entry with a `session_id` or a `local_id` is tried again after a network error, a 429 or a 5xx answer, because sending it again updates it instead of adding a second one. An entry without either is sent once.
- An agent that knows its model calls can send them as `messages`, and split the entry by them with `splits`. Then a session that telemetry also reported is counted once, call by call. Money goes as a number or a decimal string such as `"0.031"`.

Token entries sent this way arrive submitted. They never go on a token sheet.

Needs Node.js 22 or newer, or any runtime with `fetch`. No dependencies. MIT license.
