import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  bar,
  headline,
  money,
  reportTable,
  reportText,
  sessionTicket,
  sheetNote,
  tagLine,
} from './format.ts'
import type { ReviewRow, Sheet } from './tc.ts'

const SHEET: Sheet = {
  organization: 'Kestrel',
  currency: 'USD',
  tokenEntries: 5,
  cost: 212.4,
  shareOnTickets: 0.91,
  dueAt: null,
  isLate: false,
}

test('the headline reads as the docs show it', () => {
  assert.equal(headline(SHEET, 'KD-214'), '$212 spent · 91% on tickets · KD-214')
  assert.equal(headline({ ...SHEET, cost: 9.5, shareOnTickets: null }, null), '$9.50 spent')
  assert.equal(headline({ ...SHEET, currency: 'EUR' }, null), '€212 spent · 91% on tickets')
})

test('money shows cents below 100 and on request', () => {
  assert.equal(money(41.87, 'USD'), '$41.87')
  assert.equal(money(1234.5, 'USD'), '$1,235')
  assert.equal(money(1234.5, 'USD', true), '$1,234.50')
  assert.equal(money(12, 'CHF'), '12.00 CHF')
})

test('the token sheet note says late, due soon, or nothing', () => {
  const now = Date.parse('2026-10-14T10:00:00')
  assert.equal(sheetNote({ ...SHEET, isLate: true }, now), 'token sheet late')
  assert.equal(sheetNote({ ...SHEET, dueAt: '2026-10-16T17:00:00' }, now), 'token sheet due Fri')
  assert.equal(sheetNote({ ...SHEET, dueAt: '2026-10-30T17:00:00' }, now), null)
  assert.equal(sheetNote({ ...SHEET, dueAt: '2026-10-13T17:00:00' }, now), 'token sheet late')
  assert.equal(sheetNote(SHEET, now), null)
})

test('the session ticket comes from its review row', () => {
  const rows: ReviewRow[] = [
    {
      number: 1,
      sessionId: 'a',
      startedAt: null,
      cost: 1,
      tags: { ticket: 'KD-1' },
      by: '',
      summary: '',
    },
    { number: 2, sessionId: 'b', startedAt: null, cost: 1, tags: {}, by: '', summary: '' },
  ]
  assert.equal(sessionTicket(rows, 'a'), 'KD-1')
  assert.equal(sessionTicket(rows, 'b'), null)
  assert.equal(sessionTicket(rows, 'c'), null)
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
    { value: 'KD-231', cost: 80.25, share: 0.4 },
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
      '| KD-231 | $80.25 | 40% |',
      '| **total** | **$200.75** | |',
    ].join('\n'),
  )
  assert.equal(reportTable({ ...REPORT, rows: [] }), 'No cost by ticket yet.')
})

test('a report as text with bars for /tc report', () => {
  const lines = reportText(REPORT, 50).split('\n')
  assert.equal(lines.length, 5)
  assert.equal(lines[0], 'Cost by ticket')
  assert.match(lines[2] ?? '', /^KD-214 {2}\$120\.50 {2} 60% {2}█+$/)
  assert.match(lines[4] ?? '', /^total {3}\$200\.75$/)
})
