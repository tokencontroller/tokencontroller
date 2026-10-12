import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseCommand, sessionRecord } from './records.ts'

test('a session record carries the summary and the tags set by hand', () => {
  assert.deepEqual(
    sessionRecord('s', 'Fixed the cart total', { ticket: 'KD-214', purpose: 'spike' }),
    {
      agent_harness: 'claude-code',
      session_id: 's',
      summary: 'Fixed the cart total',
      ticket: 'KD-214',
      tags: { purpose: 'spike' },
    },
  )
})

test('a session record leaves out what is not known yet', () => {
  assert.deepEqual(sessionRecord('s', null, {}), { agent_harness: 'claude-code', session_id: 's' })
  assert.deepEqual(sessionRecord('s', null, { team: 'web' }), {
    agent_harness: 'claude-code',
    session_id: 's',
    tags: { team: 'web' },
  })
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
