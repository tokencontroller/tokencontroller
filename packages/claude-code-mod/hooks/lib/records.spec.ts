import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  addToOutbox,
  OUTBOX_LIMIT,
  parseCommand,
  repositoryName,
  ticketKeyCandidates,
  turnRecord,
} from './records.ts'

const USAGE = {
  model: 'claude-opus-5',
  input_tokens: 10,
  output_tokens: 5,
  cache_read_input_tokens: 100,
  cache_creation_input_tokens: 20,
}

const FACTS = {
  sessionId: 's',
  turnId: 't',
  agentId: undefined,
  at: Date.parse('2026-10-12T09:00:00Z'),
  usage: USAGE,
  costBefore: 1.5,
  costAfter: 1.8,
  cwd: '/work',
  branch: 'main',
  repository: 'kestrel/storefront',
  ticketKeys: ['KD-1', 'KD-1'],
}

test('a turn record maps Claude Code’s usage to token kinds', () => {
  assert.deepEqual(turnRecord(FACTS), {
    kind: 'turn',
    session_id: 's',
    turn_id: 't',
    at: '2026-10-12T09:00:00.000Z',
    model: 'claude-opus-5',
    tokens: { input: 10, output: 5, cache_write: 20, cache_read: 100 },
    reported_cost: 0.3,
    cwd: '/work',
    branch: 'main',
    repository: 'kestrel/storefront',
    ticket_keys: ['KD-1'],
  })
})

test('a subagent turn carries its agent; no cost figure stays null', () => {
  const record = turnRecord({ ...FACTS, agentId: 'a-1', costAfter: undefined })
  assert.equal(record.agent_id, 'a-1')
  assert.equal(record.reported_cost, null)
})

test('the first turn costs what the session cost so far', () => {
  assert.equal(turnRecord({ ...FACTS, costBefore: undefined, costAfter: 0.42 }).reported_cost, 0.42)
})

test('ticket key candidates are keys, never the prompt', () => {
  assert.deepEqual(
    ticketKeyCandidates('Fix KD-214 and OPS-3, see storefront#12; not a-1 or ISO-8601x'),
    ['KD-214', 'OPS-3', 'storefront#12'],
  )
  assert.deepEqual(ticketKeyCandidates('nothing here'), [])
})

test('repository names come from every spelling of origin', () => {
  assert.equal(repositoryName('git@github.com:kestrel/storefront.git'), 'kestrel/storefront')
  assert.equal(repositoryName('https://github.com/kestrel/storefront'), 'kestrel/storefront')
  assert.equal(
    repositoryName('https://dev.azure.com/kestrel/web/_git/storefront'),
    '_git/storefront',
  )
  assert.equal(repositoryName(null), null)
})

test('the outbox keeps the newest lines past its limit', () => {
  const full = Array.from({ length: OUTBOX_LIMIT }, (_, i) => `line ${i}`)
  const next = addToOutbox(full, { kind: 'summary', session_id: 's', summary: 'x' })
  assert.equal(next.length, OUTBOX_LIMIT)
  assert.equal(next[0], 'line 1')
  assert.equal(next.at(-1), '{"kind":"summary","session_id":"s","summary":"x"}')
})

test('/tc reads its words', () => {
  assert.deepEqual(parseCommand(''), { kind: 'open' })
  assert.deepEqual(parseCommand('ticket KD-214'), { kind: 'tags', tags: { ticket: 'KD-214' } })
  assert.deepEqual(parseCommand('tag purpose=spike team=web'), {
    kind: 'tags',
    tags: { purpose: 'spike', team: 'web' },
  })
  assert.deepEqual(parseCommand('report team'), { kind: 'report', by: 'team' })
  assert.deepEqual(parseCommand('report'), { kind: 'report', by: 'ticket' })
  assert.deepEqual(parseCommand('submit'), { kind: 'submit' })
  assert.equal(parseCommand('ticket').kind, 'help')
  assert.equal(parseCommand('tag purpose').kind, 'help')
  assert.deepEqual(parseCommand('budget'), { kind: 'help', problem: '/tc has no "budget".' })
})
