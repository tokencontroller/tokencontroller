import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  bar,
  headline,
  money,
  monthName,
  placedByLine,
  placeLine,
  reportTable,
  reportText,
  sessionTicket,
  sheetNote,
  submittedText,
  tagLine,
} from './format.ts'
import type { ReviewRow, Sheet } from './tc.ts'

const SHEET: Sheet = {
  isConnected: true,
  organization: 'Kestrel',
  currency: 'USD',
  today: 41.87,
  shareOnTickets: 0.91,
  isLate: false,
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
  placedBy: 'rule: ticket key in branch',
  candidates: [],
  splits: [{ share: 1, tags: { ticket: 'KD-1', team: 'web' } }],
}

test('the headline: today, the share on tickets, the session’s ticket', () => {
  assert.equal(headline(SHEET, 'KD-214'), '$41.87 today · 91% on tickets · KD-214')
  assert.equal(headline({ ...SHEET, today: 212.4, shareOnTickets: null }, null), '$212 today')
  assert.equal(headline({ ...SHEET, currency: 'EUR' }, null), '€41.87 today · 91% on tickets')
})

test('money shows cents below 100 and on request', () => {
  assert.equal(money(41.87, 'USD'), '$41.87')
  assert.equal(money(1234.5, 'USD'), '$1,235')
  assert.equal(money(1234.5, 'USD', true), '$1,234.50')
  assert.equal(money(12, 'CHF'), '12.00 CHF')
})

test('the token sheet note: late, and the controller’s request to submit', () => {
  assert.equal(sheetNote(SHEET), null)
  assert.equal(sheetNote({ ...SHEET, isLate: true }), 'token sheet late')
  const asked = { period: '2026-09', askedBy: 'Nadia Berger' }
  assert.equal(
    sheetNote({ ...SHEET, askedToSubmit: asked }),
    'Nadia Berger asks you to submit for September 2026',
  )
  assert.equal(
    sheetNote({ ...SHEET, isLate: true, askedToSubmit: { ...asked, askedBy: null } }),
    'token sheet late · asked to submit for September 2026',
  )
})

test('months read as words', () => {
  assert.equal(monthName('2026-09'), 'September 2026')
  assert.equal(monthName('2026-12'), 'December 2026')
  assert.equal(monthName('2026-13'), '2026-13')
  assert.equal(monthName('Q3'), 'Q3')
})

test('the session ticket comes from its review row, split or not', () => {
  const rows: ReviewRow[] = [
    ROW,
    { ...ROW, number: 2, sessionId: 'b', splits: [{ share: 1, tags: {} }] },
    {
      ...ROW,
      number: 3,
      sessionId: 'c',
      splits: [
        { share: 0.6, tags: { ticket: 'KD-231' } },
        { share: 0.4, tags: { ticket: 'KD-240' } },
      ],
    },
  ]
  assert.equal(sessionTicket(rows, 'a'), 'KD-1')
  assert.equal(sessionTicket(rows, 'b'), null)
  assert.equal(sessionTicket(rows, 'c'), 'KD-231 +1')
  assert.equal(sessionTicket(rows, 'd'), null)
})

test('where an entry sits, and what placed it', () => {
  assert.equal(placeLine(ROW), 'ticket KD-1 · team web')
  assert.equal(placeLine({ ...ROW, splits: [] }), 'no tags')
  assert.equal(
    placeLine({
      ...ROW,
      splits: [
        { share: 0.6, tags: { ticket: 'KD-231' } },
        { share: 0.4, tags: { team: 'web' } },
      ],
    }),
    'KD-231 60% · team web 40%',
  )
  assert.equal(placedByLine(ROW), 'placed by rule: ticket key in branch')
  assert.equal(
    placedByLine({ ...ROW, placedBy: 'none', candidates: ['KD-3', 'KD-4'] }),
    'not placed · maybe KD-3, KD-4',
  )
  assert.equal(
    placedByLine({ ...ROW, isRejected: true, rejectedReason: 'KD-9 is closed' }),
    'rejected: KD-9 is closed',
  )
})

test('tags read ticket, project, then the rest', () => {
  assert.equal(
    tagLine({ team: 'web', ticket: 'KD-214', client: 'acme' }),
    'ticket KD-214 · client acme · team web',
  )
})

test('bars fill eighths of a cell', () => {
  assert.equal(bar(10, 10, 4), '████')
  assert.equal(bar(5, 10, 4), '██')
  assert.equal(bar(1, 10, 2), '▎')
  assert.equal(bar(0, 10, 4), '')
})

const REPORT = {
  by: 'ticket',
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
      '| ticket | cost | share |',
      '| --- | ---: | ---: |',
      '| KD-214 | $120.50 | 60% |',
      '| (no ticket) | $80.25 | 40% |',
      '| **total** | **$200.75** | |',
    ].join('\n'),
  )
  assert.equal(reportTable({ ...REPORT, rows: [] }), 'No cost by ticket yet.')
})

test('a report as text with bars for /tc report', () => {
  const lines = reportText(REPORT, 50).split('\n')
  assert.equal(lines.length, 5)
  assert.equal(lines[0], 'Cost by ticket')
  assert.match(lines[2] ?? '', /^KD-214 {7}\$120\.50 {2} 60% {2}█+$/)
  assert.match(lines[3] ?? '', /^\(no ticket\) {3}\$80\.25 {2} 40% {2}█+[\u2589-\u258F]?$/)
  assert.match(lines[4] ?? '', /^total {8}\$200\.75$/)
})

test('what tc submit did, in one line', () => {
  assert.equal(
    submittedText({ count: 5, cost: 212.4, currency: 'USD', shareOnTickets: 0.95 }),
    'Submitted 5 token entries · $212.40 · 95% on tickets',
  )
  assert.equal(
    submittedText({ count: 0, cost: null, currency: 'USD', shareOnTickets: null }),
    'Nothing to submit.',
  )
})
