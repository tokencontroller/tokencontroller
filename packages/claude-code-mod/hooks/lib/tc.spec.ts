import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  parseJson,
  readReport,
  readReview,
  readRules,
  readSheet,
  readSubmitted,
  reason,
} from './tc.ts'

// Shapes as `tc --json` prints them (handoff of the thread "CLI and releases").
const STATUS = {
  connected: true,
  service: 'https://app.tokencontroller.com',
  organization: 'Kestrel Digital',
  person: 'm.ortner@kestrel.example',
  agents: [{ agent_harness: 'claude-code', sessions: 4 }],
  rules: 3,
  today: { list_value: '12.40', currency: 'EUR', share_on_tickets_percent: 80 },
  token_sheet: { period_start: '2026-10-05', period_end: '2026-10-11', late: true },
}

const ENTRY = {
  number: 5,
  agent_harness: 'claude-code',
  session_id: '6b1f',
  model: 'claude-opus-5',
  started_at: '2026-10-11T09:12:00Z',
  ended_at: '2026-10-11T11:40:00Z',
  tokens: { input: 4120, output: 610, cache_write: 0, cache_read: 18200 },
  list_value: '41.87',
  currency: 'USD',
  summary: 'Fixed checkout redirect loop',
  status: 'draft',
  rejected_reason: null,
  placed_by: 'rule: ticket key in branch',
  candidates: ['KD-231', 'KD-240'],
  splits: [{ share_percent: 100, list_value: '41.87', tags: { ticket: 'KD-214', team: 'web' } }],
}

test('reads tc status', () => {
  assert.deepEqual(readSheet(STATUS), {
    isConnected: true,
    organization: 'Kestrel Digital',
    currency: 'EUR',
    today: 12.4,
    shareOnTickets: 0.8,
    isLate: true,
  })
})

test('an unconnected or unreadable status reads as local with nothing known', () => {
  assert.deepEqual(readSheet({ connected: false, organization: null, token_sheet: null }), {
    isConnected: false,
    organization: null,
    currency: 'USD',
    today: 0,
    shareOnTickets: null,
    isLate: false,
  })
  assert.deepEqual(readSheet(undefined), readSheet({}))
})

test('reads the review list', () => {
  assert.deepEqual(readReview({ token_entries: [ENTRY, { list_value: '1' }] }), [
    {
      number: 5,
      sessionId: '6b1f',
      startedAt: '2026-10-11T09:12:00Z',
      cost: 41.87,
      summary: 'Fixed checkout redirect loop',
      isRejected: false,
      rejectedReason: null,
      placedBy: 'rule: ticket key in branch',
      candidates: ['KD-231', 'KD-240'],
      splits: [{ share: 1, tags: { ticket: 'KD-214', team: 'web' } }],
    },
  ])
  assert.deepEqual(readReview('nonsense'), [])
})

test('a rejected entry and a model without a price', () => {
  const [row] = readReview({
    token_entries: [
      { ...ENTRY, status: 'rejected', rejected_reason: 'KD-999 does not exist', list_value: null },
    ],
  })
  assert.equal(row?.isRejected, true)
  assert.equal(row?.rejectedReason, 'KD-999 does not exist')
  assert.equal(row?.cost, null)
})

test('reads a report: decimal strings, no value, shares of the total', () => {
  assert.deepEqual(
    readReport(
      {
        by: 'team',
        currency: 'USD',
        rows: [
          { value: 'web', list_value: '30.00', token_entries: 3 },
          { value: null, list_value: '10.00', token_entries: 1 },
          { value: 'broken' },
        ],
        total: '40.00',
      },
      'ticket',
    ),
    {
      by: 'team',
      currency: 'USD',
      rows: [
        { value: 'web', cost: 30, share: 0.75 },
        { value: null, cost: 10, share: 0.25 },
      ],
      total: 40,
    },
  )
})

test('reads rules by their text and place', () => {
  assert.deepEqual(
    readRules({
      rules: [
        { number: 1, text: 'branch ^review/ -> purpose=code-review' },
        { name: 'ticket key in branch' },
        { when: {} },
      ],
    }),
    [
      { number: 1, text: 'branch ^review/ -> purpose=code-review' },
      { number: 2, text: 'ticket key in branch' },
    ],
  )
})

test('reads what tc submit did', () => {
  assert.deepEqual(
    readSubmitted({
      submitted: 5,
      list_value: '212.40',
      currency: 'USD',
      share_on_tickets_percent: 95,
    }),
    { count: 5, cost: 212.4, currency: 'USD', shareOnTickets: 0.95 },
  )
})

test('the reason is tc’s first line', () => {
  assert.equal(reason('tc: not connected\nmore', '', 'failed'), 'tc: not connected')
  assert.equal(reason('', '', 'failed'), 'failed')
  assert.equal(parseJson('{'), undefined)
})
