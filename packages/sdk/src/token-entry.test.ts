import assert from 'node:assert/strict'
import { test } from 'node:test'

import { problems, type TokenEntry } from './token-entry.ts'

const ENTRY: TokenEntry = {
  agent_harness: 'claude-code',
  model: 'claude-opus-5',
  started_at: '2026-09-18T09:00:00Z',
  ended_at: '2026-09-18T10:00:00Z',
  tokens: { input: 10, output: 2 },
}

test('a complete entry has no problems', () => {
  assert.deepEqual(problems(ENTRY), [])
})

test('names every missing required field', () => {
  const entry = { tokens: { input: 1 } } as unknown as TokenEntry
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

test('token counts are whole, known and not empty', () => {
  assert.deepEqual(problems({ ...ENTRY, tokens: {} }), ['tokens needs at least one kind'])
  assert.deepEqual(problems({ ...ENTRY, tokens: { input: 1.5 } }), [
    'tokens.input must be a whole number, 0 or more',
  ])
  const unknownKind = { ...ENTRY, tokens: { input: 1, reasoning: 3 } as TokenEntry['tokens'] }
  assert.deepEqual(problems(unknownKind), [
    'tokens.reasoning is not a token kind (input, output, cache_write, cache_read)',
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
  assert.deepEqual(problems({ ...ENTRY, ticket: 'KD-1', tags: { ticket: 'KD-2' } }), [
    'ticket and tags.ticket differ: send one',
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
  tokens: { input: 10, output: 2 },
  messages: [
    { message_id: 'm1', at: '2026-09-18T09:10:00Z', tokens: { input: 6, output: 1 } },
    {
      message_id: 'm2',
      at: '2026-09-18T09:40:00Z',
      tokens: { input: 4, output: 1 },
      reported_cost: '0.01',
    },
  ],
}

test('messages belong to a session and add up to the entry', () => {
  assert.deepEqual(problems(MESSAGES), [])
  const { session_id: _, ...withoutSession } = MESSAGES
  assert.deepEqual(problems(withoutSession), ['messages need the session_id they belong to'])
  assert.deepEqual(problems({ ...MESSAGES, tokens: { input: 11, output: 2 } }), [
    "tokens must equal the sum of the messages' tokens",
  ])
})

test('splits are segments by message or shares by hand', () => {
  const segments = [
    { message_ids: ['m1'], tags: { ticket: 'KD-1' } },
    { message_ids: ['m2'], tags: { ticket: 'KD-2' } },
  ]
  assert.deepEqual(problems({ ...MESSAGES, splits: segments }), [])
  assert.deepEqual(problems({ ...MESSAGES, splits: segments.slice(0, 1) }), [
    'splits by message must hold every message of the entry exactly once',
  ])
  assert.deepEqual(
    problems({ ...ENTRY, splits: [{ share_percent: 60 }, { share_percent: 40 }] }),
    [],
  )
  assert.deepEqual(problems({ ...ENTRY, splits: [{ share_percent: 60 }, { share_percent: 30 }] }), [
    'split shares must add up to 100 percent',
  ])
  assert.deepEqual(problems({ ...MESSAGES, splits: [{ share_percent: 100 }, segments[0] ?? {}] }), [
    'splits are either all by message_ids or all by share_percent',
  ])
})
