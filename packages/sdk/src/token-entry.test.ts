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
    'reported_cost must be a number, 0 or more',
  ])
  const tags = { team: ['web', 'api'] } as unknown as Record<string, string>
  assert.deepEqual(problems({ ...ENTRY, tags }), ['tags.team must be one text value'])
  assert.deepEqual(problems({ ...ENTRY, signals: { pull_request: 0 } }), [
    'signals.pull_request must be a pull request number',
  ])
})
