import assert from 'node:assert/strict'
import { test } from 'node:test'
import { countTurn, NO_FACTS, parseCommand, sessionRecord, signInSignals } from './records.ts'

test('a session record carries the summary, the facts and the tags set by hand', () => {
  const facts = countTurn(countTurn(NO_FACTS, 4000, false), 1500, true)
  assert.deepEqual(
    sessionRecord('s', 'Fixed the cart total', facts, { feature: 'KD-214', purpose: 'spike' }),
    {
      agent_harness: 'claude-code',
      session_id: 's',
      summary: 'Fixed the cart total',
      facts: { turns: 1, duration_seconds: 6, subagents: 1 },
      feature: 'KD-214',
      tags: { purpose: 'spike' },
    },
  )
})

test('a session record leaves out what is not known yet', () => {
  assert.deepEqual(sessionRecord('s', null, NO_FACTS, {}), {
    agent_harness: 'claude-code',
    session_id: 's',
  })
  assert.deepEqual(sessionRecord('s', null, NO_FACTS, { team: 'web' }), {
    agent_harness: 'claude-code',
    session_id: 's',
    tags: { team: 'web' },
  })
})

test('the sign-in at session start goes on the record as signals', () => {
  assert.deepEqual(signInSignals({ kind: 'claude.ai', organizationId: 'org-1' }), {
    sign_in: 'claude.ai',
    'organization.id': 'org-1',
  })
  assert.deepEqual(signInSignals({ kind: 'api-key' }), { sign_in: 'api-key' })
  assert.deepEqual(signInSignals({ kind: 'none' }), {})
  assert.deepEqual(signInSignals({ deny: 'not installed by managed settings' }), {})
  assert.deepEqual(sessionRecord('s', null, NO_FACTS, {}, { sign_in: 'bedrock' }), {
    agent_harness: 'claude-code',
    session_id: 's',
    signals: { sign_in: 'bedrock' },
  })
  assert.deepEqual(sessionRecord('s', null, NO_FACTS, {}, {}), {
    agent_harness: 'claude-code',
    session_id: 's',
  })
})

test('/tc reads its words', () => {
  assert.deepEqual(parseCommand(''), { kind: 'open' })
  assert.deepEqual(parseCommand('feature KD-214'), { kind: 'tags', tags: { feature: 'KD-214' } })
  assert.deepEqual(parseCommand('tag purpose=spike team=web'), {
    kind: 'tags',
    tags: { purpose: 'spike', team: 'web' },
  })
  assert.deepEqual(parseCommand('report team'), { kind: 'report', by: 'team' })
  assert.deepEqual(parseCommand('report'), { kind: 'report', by: 'feature' })
  assert.deepEqual(parseCommand('submit'), { kind: 'submit' })
  assert.equal(parseCommand('feature').kind, 'help')
  assert.equal(parseCommand('tag purpose').kind, 'help')
  assert.deepEqual(parseCommand('ticket KD-1'), { kind: 'help', problem: '/tc has no "ticket".' })
})
