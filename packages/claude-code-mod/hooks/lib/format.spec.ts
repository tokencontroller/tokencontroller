import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  assignedLine,
  bar,
  dateName,
  headline,
  money,
  reportTable,
  reportText,
  sessionFeature,
  splitLine,
  statusNote,
  submittedText,
  tagLine,
} from './format.ts'
import type { ReviewRow, Status } from './tc.ts'

const STATUS: Status = {
  isConnected: true,
  organization: 'Kestrel',
  currency: 'USD',
  today: 41.87,
  shareOnFeatures: 0.91,
  askedToSubmit: null,
}

const ROW: ReviewRow = {
  number: 1,
  sessionId: 'a',
  startedAt: null,
  cost: 1,
  summary: '',
  isRejected: false,
  rejectedReason: null,
  assignedBy: 'rule: key in branch',
  label: 'direct',
  splits: [{ fraction: 1, tags: { feature: 'KD-1', team: 'web' } }],
}

test('the headline: today, the share on features, the session’s feature', () => {
  assert.equal(headline(STATUS, 'KD-214'), '$41.87 today · 91% on features · KD-214')
  assert.equal(headline({ ...STATUS, today: 212.4, shareOnFeatures: null }, null), '$212 today')
  assert.equal(headline({ ...STATUS, currency: 'EUR' }, null), '€41.87 today · 91% on features')
})

test('money shows cents below 100 and on request', () => {
  assert.equal(money(41.87, 'USD'), '$41.87')
  assert.equal(money(1234.5, 'USD'), '$1,235')
  assert.equal(money(1234.5, 'USD', true), '$1,234.50')
  assert.equal(money(12, 'CHF'), '12.00 CHF')
})

test('the note: the org manager’s request to submit before the cutoff moves', () => {
  assert.equal(statusNote(STATUS), null)
  const asked = { cutoff: '2026-09-30', askedBy: 'Nadia Berger' }
  assert.equal(
    statusNote({ ...STATUS, askedToSubmit: asked }),
    'Nadia Berger asks you to submit before the cutoff moves to 30 September 2026',
  )
  assert.equal(
    statusNote({ ...STATUS, askedToSubmit: { ...asked, askedBy: null } }),
    'asked to submit before the cutoff moves to 30 September 2026',
  )
})

test('dates read as words', () => {
  assert.equal(dateName('2026-09-30'), '30 September 2026')
  assert.equal(dateName('2026-12-01'), '1 December 2026')
  assert.equal(dateName('2026-13-01'), '2026-13-01')
  assert.equal(dateName('Q3'), 'Q3')
})

test('the session feature comes from its check list row, split or not', () => {
  const rows: ReviewRow[] = [
    ROW,
    { ...ROW, number: 2, sessionId: 'b', splits: [{ fraction: 1, tags: {} }] },
    {
      ...ROW,
      number: 3,
      sessionId: 'c',
      splits: [
        { fraction: 0.6, tags: { feature: 'KD-231' } },
        { fraction: 0.4, tags: { feature: 'KD-240' } },
      ],
    },
  ]
  assert.equal(sessionFeature(rows, 'a'), 'KD-1')
  assert.equal(sessionFeature(rows, 'b'), null)
  assert.equal(sessionFeature(rows, 'c'), 'KD-231 +1')
  assert.equal(sessionFeature(rows, 'd'), null)
})

test('where a session sits, and what assigned it', () => {
  assert.equal(splitLine(ROW), 'feature KD-1 · team web')
  assert.equal(splitLine({ ...ROW, splits: [] }), 'no project')
  assert.equal(
    splitLine({
      ...ROW,
      splits: [
        { fraction: 0.6, tags: { feature: 'KD-231' } },
        { fraction: 0.4, tags: { team: 'web' } },
      ],
    }),
    'KD-231 60% · team web 40%',
  )
  assert.equal(assignedLine(ROW), 'rule: key in branch · direct')
  assert.equal(
    assignedLine({ ...ROW, assignedBy: null, label: 'organization overhead' }),
    'organization overhead',
  )
  assert.equal(assignedLine({ ...ROW, assignedBy: null, label: null }), 'no rule matched')
  assert.equal(
    assignedLine({ ...ROW, isRejected: true, rejectedReason: 'KD-9 is closed' }),
    'rejected: KD-9 is closed',
  )
})

test('tags read feature, project, then the rest', () => {
  assert.equal(
    tagLine({ team: 'web', feature: 'KD-214', client: 'acme' }),
    'feature KD-214 · client acme · team web',
  )
})

test('bars fill eighths of a cell', () => {
  assert.equal(bar(10, 10, 4), '████')
  assert.equal(bar(5, 10, 4), '██')
  assert.equal(bar(1, 10, 2), '▎')
  assert.equal(bar(0, 10, 4), '')
})

const REPORT = {
  by: 'feature',
  currency: 'USD',
  rows: [
    { value: 'KD-214', cost: 120.5, share: 0.6 },
    { value: null, cost: 80.25, share: 0.4 },
  ],
  total: 200.75,
}

test('a report as a Markdown table for the model', () => {
  assert.equal(
    reportTable(REPORT),
    [
      '| feature | cost | share |',
      '| --- | ---: | ---: |',
      '| KD-214 | $120.50 | 60% |',
      '| (no feature) | $80.25 | 40% |',
      '| **total** | **$200.75** | |',
    ].join('\n'),
  )
  assert.equal(reportTable({ ...REPORT, rows: [] }), 'No cost by feature yet.')
})

test('a report as text with bars for /tc report', () => {
  const lines = reportText(REPORT, 50).split('\n')
  assert.equal(lines.length, 5)
  assert.equal(lines[0], 'Cost by feature')
  assert.match(lines[2] ?? '', /^KD-214 {8}\$120\.50 {2} 60% {2}█+$/)
  assert.match(lines[3] ?? '', /^\(no feature\) {3}\$80\.25 {2} 40% {2}█+[▉-▏]?$/)
  assert.match(lines[4] ?? '', /^total {9}\$200\.75$/)
})

test('what tc submit did, in one line', () => {
  assert.equal(
    submittedText({ count: 5, cost: 212.4, currency: 'USD', shareOnFeatures: 0.95 }),
    'Submitted 5 sessions · $212.40 · 95% on features',
  )
  assert.equal(
    submittedText({ count: 0, cost: null, currency: 'USD', shareOnFeatures: null }),
    'Nothing to submit.',
  )
})
