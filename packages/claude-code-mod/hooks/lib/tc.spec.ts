import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseJson, readReport, readReview, readRules, readSheet, reason } from './tc.ts'

test('reads tc status, with unknowns left unknown', () => {
  assert.deepEqual(
    readSheet({
      organization: 'Kestrel',
      currency: 'EUR',
      token_sheet: {
        token_entries: 5,
        cost: '212.40',
        share_on_tickets: 0.91,
        due_at: null,
        is_late: true,
      },
    }),
    {
      organization: 'Kestrel',
      currency: 'EUR',
      tokenEntries: 5,
      cost: 212.4,
      shareOnTickets: 0.91,
      dueAt: null,
      isLate: true,
    },
  )
  assert.deepEqual(readSheet(undefined), {
    organization: null,
    currency: 'USD',
    tokenEntries: 0,
    cost: 0,
    shareOnTickets: null,
    dueAt: null,
    isLate: false,
  })
})

test('reads the review list as a list or under token_entries', () => {
  const row = {
    number: 3,
    session_id: 's',
    started_at: 'x',
    cost: 1.5,
    tags: { ticket: 'KD-1', n: 2 },
    by: 'rule',
  }
  const expected = [
    {
      number: 3,
      sessionId: 's',
      startedAt: 'x',
      cost: 1.5,
      tags: { ticket: 'KD-1' },
      by: 'rule',
      summary: '',
    },
  ]
  assert.deepEqual(readReview([row, { cost: 1 }]), expected)
  assert.deepEqual(readReview({ token_entries: [row] }), expected)
  assert.deepEqual(readReview('nonsense'), [])
})

test('reads a report and adds the total when tc leaves it out', () => {
  const report = readReport(
    {
      rows: [
        { value: 'KD-1', cost: 2 },
        { name: 'KD-2', cost: 3, share: 0.6 },
      ],
    },
    'ticket',
  )
  assert.deepEqual(report, {
    by: 'ticket',
    currency: 'USD',
    rows: [
      { value: 'KD-1', cost: 2, share: null },
      { value: 'KD-2', cost: 3, share: 0.6 },
    ],
    total: 5,
  })
})

test('reads rules', () => {
  assert.deepEqual(
    readRules([{ number: 1, rule: 'branch ^review/ → purpose=code-review' }, { number: 2 }]),
    [{ number: 1, rule: 'branch ^review/ → purpose=code-review' }],
  )
})

test('the reason is tc’s first line', () => {
  assert.equal(reason('tc: not connected\nmore', '', 'failed'), 'tc: not connected')
  assert.equal(reason('', '', 'failed'), 'failed')
  assert.equal(parseJson('{'), undefined)
})
