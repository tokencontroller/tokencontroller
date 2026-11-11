import assert from 'node:assert/strict'
import { test } from 'node:test'

import { problems, type TokenEntry } from './token-entry.ts'

const ENTRY: TokenEntry = {
  agent_harness: 'claude-code',
  model: 'claude-opus-5',
  started_at: '2026-09-18T09:00:00Z',
  ended_at: '2026-09-18T10:00:00Z',
  tokens: { input: 10, output: 2, cache_read: 0, cache_write: 0 },
}

test('a complete entry has no problems', () => {
  assert.deepEqual(problems(ENTRY), [])
})

test('names every missing required field', () => {
  const entry = { tokens: ENTRY.tokens } as unknown as TokenEntry
  assert.deepEqual(problems(entry), [
    'agent_harness is required',
    'model is required',
    'started_at is required',
    'ended_at is required',
  ])
})

test('times must be timestamps in order', () => {
  assert.deepEqual(problems({ ...ENTRY, started_at: 'yesterday' }), [
    'started_at is not a timestamp',
  ])
  assert.deepEqual(problems({ ...ENTRY, ended_at: '2026-09-18T08:00:00Z' }), [
    'ended_at is before started_at',
  ])
})

test('token kinds are open names with whole counts, at least one', () => {
  assert.deepEqual(problems({ ...ENTRY, tokens: { input: 1, output: 2 } }), [])
  assert.deepEqual(problems({ ...ENTRY, tokens: {} }), [
    'tokens needs at least one kind, such as input',
  ])
  assert.deepEqual(problems({ ...ENTRY, tokens: { ...ENTRY.tokens, input: 1.5 } }), [
    'tokens.input must be a whole number, 0 or more',
  ])
  assert.deepEqual(
    problems({ ...ENTRY, tokens: { ...ENTRY.tokens, reasoning: 3, cache_write_1h: 4 } }),
    [],
  )
  assert.deepEqual(problems({ ...ENTRY, tokens: { ...ENTRY.tokens, 'Cache Write': 3 } }), [
    'tokens.Cache Write is not a token kind: small letters, digits and _',
  ])
})

test('the provider is spelled as bills spell it, and any signal goes by its name', () => {
  assert.deepEqual(problems({ ...ENTRY, provider: 'aws_bedrock' }), [])
  assert.deepEqual(problems({ ...ENTRY, provider: 'AWS Bedrock' }), [
    'provider is spelled as bills spell it: small letters, digits and _, such as aws_bedrock',
  ])
  assert.deepEqual(
    problems({ ...ENTRY, signals: { 'organization.id': 'org-1', sign_in: 'claude.ai' } }),
    [],
  )
  assert.deepEqual(problems({ ...ENTRY, signals: { notes: 'x'.repeat(10_001) } }), [
    'signals.notes is too long: send a value, not a transcript',
  ])
})

test('facts are counts, a duration and a result pointer', () => {
  assert.deepEqual(
    problems({
      ...ENTRY,
      facts: {
        turns: 4,
        tool_calls: 12,
        duration_seconds: 310.5,
        result: 'https://github.com/kestrel/shop/pull/88',
      },
    }),
    [],
  )
  assert.deepEqual(problems({ ...ENTRY, facts: { turns: -1, result: '' } }), [
    'facts.turns must be a number, 0 or more',
    'facts.result must be a URL or another pointer, as text',
  ])
})

test('cost, tags and pull request are checked', () => {
  assert.deepEqual(problems({ ...ENTRY, reported_cost: -1 }), [
    'reported_cost must be an amount, 0 or more',
  ])
  assert.deepEqual(problems({ ...ENTRY, reported_cost: '0.031' }), [])
  assert.deepEqual(problems({ ...ENTRY, reported_cost: '1,20' }), [
    'reported_cost must be an amount, 0 or more',
  ])
  assert.deepEqual(problems({ ...ENTRY, tags: { Team: 'web', person: 'mara' } }), [
    'tags.Team: a tag name is lowercase letters, digits, _ or -',
    'tags.person is set from the token that sends the entry',
  ])
  assert.deepEqual(problems({ ...ENTRY, feature: 'KD-1', tags: { feature: 'KD-2' } }), [
    'feature and tags.feature differ: send one',
  ])
  const tags = { team: ['web', 'api'] } as unknown as Record<string, string>
  assert.deepEqual(problems({ ...ENTRY, tags }), ['tags.team must be one text value'])
  assert.deepEqual(problems({ ...ENTRY, signals: { pull_request: 0 } }), [
    'signals.pull_request must be a pull request number',
  ])
})

const MESSAGES: TokenEntry = {
  ...ENTRY,
  session_id: 's-1',
  tokens: { input: 10, output: 2, cache_read: 0, cache_write: 0 },
  messages: [
    {
      message_id: 'm1',
      at: '2026-09-18T09:10:00Z',
      tokens: { input: 6, output: 1, cache_read: 0, cache_write: 0 },
    },
    {
      message_id: 'm2',
      at: '2026-09-18T09:40:00Z',
      tokens: { input: 4, output: 1, cache_read: 0, cache_write: 0 },
      reported_cost: '0.01',
    },
  ],
}

test('messages belong to a session and add up to the entry', () => {
  assert.deepEqual(problems(MESSAGES), [])
  const { session_id: _, ...withoutSession } = MESSAGES
  assert.deepEqual(problems(withoutSession), ['messages need the session_id they belong to'])
  assert.deepEqual(problems({ ...MESSAGES, tokens: { ...MESSAGES.tokens, input: 11 } }), [
    "tokens must equal the sum of the messages' tokens",
  ])
  // A kind left out has no tokens, so a message without its zero kinds
  // still adds up; a kind the entry does not carry does not.
  const [first, second] = MESSAGES.messages ?? []
  if (first === undefined || second === undefined) throw new Error('two messages')
  const short = { ...first, tokens: { input: 6, output: 1 } }
  assert.deepEqual(problems({ ...MESSAGES, messages: [short, second] }), [])
  const reasoning = { ...first, tokens: { ...first.tokens, reasoning: 3 } }
  assert.deepEqual(problems({ ...MESSAGES, messages: [reasoning, second] }), [
    "tokens must equal the sum of the messages' tokens",
  ])
})

test('splits are by message or by fraction', () => {
  const byMessage = [
    { message_ids: ['m1'], tags: { feature: 'KD-1' } },
    { message_ids: ['m2'], tags: { feature: 'KD-2' } },
  ]
  assert.deepEqual(problems({ ...MESSAGES, splits: byMessage }), [])
  assert.deepEqual(problems({ ...MESSAGES, splits: byMessage.slice(0, 1) }), [
    'splits by message must hold every message of the entry exactly once',
  ])
  const byFraction = [
    { fraction: 0.6, tags: { feature: 'KD-1' } },
    { fraction: '0.4', tags: { feature: 'KD-2' } },
  ]
  assert.deepEqual(problems({ ...ENTRY, splits: byFraction }), [])
  assert.deepEqual(problems({ ...ENTRY, splits: [{ fraction: 0.6 }, { fraction: 0.3 }] }), [
    'split fractions must add up to 1',
  ])
  assert.deepEqual(problems({ ...ENTRY, splits: [{ fraction: 60 }, { fraction: 40 }] }), [
    'a split fraction is a number above 0 and up to 1, such as 0.6 or "0.6"',
  ])
  assert.deepEqual(problems({ ...MESSAGES, splits: [{ fraction: 1 }, byMessage[0] ?? {}] }), [
    'splits are either all by message_ids or all by fraction',
  ])
})
