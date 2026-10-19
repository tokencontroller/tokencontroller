import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  parseJson,
  readReport,
  readReview,
  readRules,
  readStatus,
  readSubmitted,
  reason,
} from './tc.ts'

// Shapes as `tc --json` prints them (docs: CLI reference, Check and submit).
const STATUS = {
  connected: true,
  service: 'https://app.tokencontroller.com',
  organization: 'Kestrel Digital',
  person: 'm.ortner@kestrel.example',
  agents: [{ agent_harness: 'claude-code', sessions: 4 }],
  rules: 3,
  today: { cost: '12.40', currency: 'EUR', share_on_features_percent: 80 },
  last_submitted_at: '2026-10-11T17:02:00Z',
  asked_to_submit: {
    cutoff: '2026-09-30',
    asked_by: 'Nadia Berger',
    asked_at: '2026-10-01T08:00:00Z',
  },
}

const SESSION = {
  number: 5,
  agent_harness: 'claude-code',
  session_id: '6b1f',
  model: 'claude-opus-5',
  started_at: '2026-10-11T09:12:00Z',
  ended_at: '2026-10-11T11:40:00Z',
  tokens: { input: 4120, output: 610, cache_read: 18200, cache_write: 0 },
  cost: '41.87',
  currency: 'USD',
  summary: 'Fixed checkout redirect loop',
  status: 'unsubmitted',
  rejected_reason: null,
  assigned_by: 'rule: key in branch',
  label: 'direct',
  splits: [{ fraction: '1', cost: '41.87', tags: { feature: 'KD-214', team: 'web' } }],
}

test('reads tc status', () => {
  assert.deepEqual(readStatus(STATUS), {
    isConnected: true,
    organization: 'Kestrel Digital',
    currency: 'EUR',
    today: 12.4,
    shareOnFeatures: 0.8,
    askedToSubmit: { cutoff: '2026-09-30', askedBy: 'Nadia Berger' },
  })
})

test('an unconnected or unreadable status reads as local with nothing known', () => {
  assert.deepEqual(readStatus({ connected: false, organization: null }), {
    isConnected: false,
    organization: null,
    currency: 'USD',
    today: 0,
    shareOnFeatures: null,
    askedToSubmit: null,
  })
  assert.deepEqual(readStatus(undefined), readStatus({}))
  assert.equal(readStatus({ asked_to_submit: { asked_by: 'Nadia' } }).askedToSubmit, null)
})

test('reads the check list', () => {
  assert.deepEqual(readReview({ sessions: [SESSION, { cost: '1' }] }), [
    {
      number: 5,
      sessionId: '6b1f',
      startedAt: '2026-10-11T09:12:00Z',
      cost: 41.87,
      summary: 'Fixed checkout redirect loop',
      isRejected: false,
      rejectedReason: null,
      assignedBy: 'rule: key in branch',
      label: 'direct',
      splits: [{ fraction: 1, tags: { feature: 'KD-214', team: 'web' } }],
    },
  ])
  assert.deepEqual(readReview('nonsense'), [])
})

test('a rejected session and a model without a price', () => {
  const [row] = readReview({
    sessions: [
      { ...SESSION, status: 'rejected', rejected_reason: 'KD-999 does not exist', cost: null },
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
          { value: 'web', cost: '30.00', sessions: 3 },
          { value: null, cost: '10.00', sessions: 1 },
          { value: 'broken' },
        ],
        total: '40.00',
      },
      'feature',
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
        { number: 1, text: 'branch starts-with review/ purpose=code-review' },
        { name: 'key in branch' },
        { when: {} },
      ],
    }),
    [
      { number: 1, text: 'branch starts-with review/ purpose=code-review' },
      { number: 2, text: 'key in branch' },
    ],
  )
})

test('reads what tc submit did', () => {
  assert.deepEqual(
    readSubmitted({
      submitted: 5,
      cost: '212.40',
      currency: 'USD',
      share_on_features_percent: 95,
    }),
    { count: 5, cost: 212.4, currency: 'USD', shareOnFeatures: 0.95 },
  )
})

test('the reason is tc’s first line', () => {
  assert.equal(reason('tc: not connected\nmore', '', 'failed'), 'tc: not connected')
  assert.equal(reason('', '', 'failed'), 'failed')
  assert.equal(parseJson('{'), undefined)
})
