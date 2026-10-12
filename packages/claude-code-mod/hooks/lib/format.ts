import type { Report, ReportRow, ReviewRow, Sheet, Submitted } from './tc.ts'

const SYMBOLS: Record<string, string> = { USD: '$', EUR: '€', GBP: '£' }

/** `$212`, `$41.87`: whole units from 100 up, cents below. */
export function money(amount: number, currency: string, exact = false): string {
  const symbol = SYMBOLS[currency]
  const digits = exact || Math.abs(amount) < 100 ? 2 : 0
  const text = amount.toLocaleString('en-US', {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })
  return symbol === undefined ? `${text} ${currency}` : `${symbol}${text}`
}

export function percent(share: number): string {
  return `${Math.round(share * 100)}%`
}

/**
 * The headline of the band and the pane: today's cost, the share on tickets,
 * and this session's ticket, such as `$41.87 today · 91% on tickets · KD-214`.
 */
export function headline(sheet: Sheet, ticket: string | null): string {
  const parts = [`${money(sheet.today, sheet.currency)} today`]
  if (sheet.shareOnTickets !== null) parts.push(`${percent(sheet.shareOnTickets)} on tickets`)
  if (ticket) parts.push(ticket)
  return parts.join(' · ')
}

const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]

/** `September 2026` for `2026-09`; anything else as tc sent it. */
export function monthName(period: string): string {
  const match = /^(\d{4})-(\d{2})$/.exec(period)
  const month = match === null ? undefined : MONTHS[Number(match[2]) - 1]
  return match === null || month === undefined ? period : `${month} ${match[1]}`
}

/**
 * The token sheet's note: late, and the controller's request to submit
 * before the close, such as `Nadia Berger asks you to submit for September 2026`.
 */
export function sheetNote(sheet: Sheet): string | null {
  const notes: string[] = []
  if (sheet.isLate) notes.push('token sheet late')
  const asked = sheet.askedToSubmit
  if (asked !== null) {
    const month = monthName(asked.period)
    notes.push(
      asked.askedBy === null
        ? `asked to submit for ${month}`
        : `${asked.askedBy} asks you to submit for ${month}`,
    )
  }
  return notes.length === 0 ? null : notes.join(' · ')
}

function tickets(row: ReviewRow): string[] {
  return [...new Set(row.splits.flatMap(split => (split.tags.ticket ? [split.tags.ticket] : [])))]
}

/** The ticket tc placed this session on, if any: `KD-214`, or `KD-231 +1` when split. */
export function sessionTicket(rows: readonly ReviewRow[], sessionId: string): string | null {
  const row = rows.find(r => r.sessionId === sessionId)
  if (row === undefined) return null
  const [first, ...rest] = tickets(row)
  if (first === undefined) return null
  return rest.length === 0 ? first : `${first} +${rest.length}`
}

/** `ticket KD-214 · team web`, the review list's tag column. */
export function tagLine(tags: Record<string, string>): string {
  const order = ['ticket', 'project']
  const names = Object.keys(tags).sort((a, b) => {
    const ia = order.indexOf(a)
    const ib = order.indexOf(b)
    return (ia === -1 ? 9 : ia) - (ib === -1 ? 9 : ib) || a.localeCompare(b)
  })
  return names.map(name => `${name} ${tags[name]}`).join(' · ')
}

/** Where a token entry sits: its tags, or each split's ticket or tags with its share. */
export function placeLine(row: ReviewRow): string {
  if (row.splits.length <= 1) return tagLine(row.splits[0]?.tags ?? {}) || 'no tags'
  return row.splits
    .map(split => {
      const place = split.tags.ticket ?? (tagLine(split.tags) || 'no tags')
      return split.share === null ? place : `${place} ${percent(split.share)}`
    })
    .join(' · ')
}

/** The line under a token entry: what placed it, or why it came back. */
export function placedByLine(row: ReviewRow): string {
  if (row.isRejected) return `rejected: ${row.rejectedReason ?? 'see the web app'}`
  const parts: string[] = []
  if (row.placedBy !== null && row.placedBy !== 'none') parts.push(`placed by ${row.placedBy}`)
  else parts.push('not placed')
  if (row.candidates.length > 0) parts.push(`maybe ${row.candidates.join(', ')}`)
  return parts.join(' · ')
}

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

/** `09:12` today, `Mon 09:12` this week, `2026-09-18` before. */
export function when(iso: string | null, now: number): string {
  if (iso === null) return ''
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return ''
  const time = `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`
  const days = (now - at.getTime()) / 86_400_000
  if (new Date(now).toDateString() === at.toDateString()) return time
  if (days < 6) return `${DAYS[at.getDay()]} ${time}`
  return at.toISOString().slice(0, 10)
}

const EIGHTHS = ['', '▏', '▎', '▍', '▌', '▋', '▊', '▉']

/** A bar of `width` cells for `value` out of `max`, in eighths of a cell. */
export function bar(value: number, max: number, width: number): string {
  if (max <= 0 || value <= 0 || width <= 0) return ''
  const eighths = Math.max(1, Math.round((Math.min(value, max) / max) * width * 8))
  return '█'.repeat(Math.floor(eighths / 8)) + EIGHTHS[eighths % 8]
}

/** A row's label: its value, or `(no ticket)` for the cost without one. */
export function rowLabel(row: ReportRow, by: string): string {
  return row.value ?? `(no ${by})`
}

/** A report as a Markdown table, for the model. */
export function reportTable(report: Report): string {
  if (report.rows.length === 0) return `No cost by ${report.by} yet.`
  const lines = [
    `| ${report.by} | cost | share |`,
    '| --- | ---: | ---: |',
    ...report.rows.map(
      row =>
        `| ${rowLabel(row, report.by)} | ${money(row.cost, report.currency, true)} | ${row.share === null ? '' : percent(row.share)} |`,
    ),
    `| **total** | **${money(report.total, report.currency, true)}** | |`,
  ]
  return lines.join('\n')
}

/** The same report as aligned text with bars, for a command's output row. */
export function reportText(report: Report, width = 72): string {
  if (report.rows.length === 0) return `No cost by ${report.by} yet.`
  const labels = report.rows.map(row => rowLabel(row, report.by))
  const label = Math.min(24, Math.max(5, ...labels.map(l => l.length)))
  const costs = report.rows.map(row => money(row.cost, report.currency, true))
  const costWidth = Math.max(...costs.map(c => c.length))
  const barWidth = Math.min(40, Math.max(8, width - label - costWidth - 10))
  const max = Math.max(...report.rows.map(row => row.cost))
  const lines = report.rows.map((row, i) => {
    const share = row.share === null ? '' : percent(row.share)
    return `${(labels[i] ?? '').slice(0, label).padEnd(label)}  ${(costs[i] ?? '').padStart(costWidth)}  ${share.padStart(4)}  ${bar(row.cost, max, barWidth)}`
  })
  return [
    `Cost by ${report.by}`,
    '',
    ...lines,
    `${'total'.padEnd(label)}  ${money(report.total, report.currency, true).padStart(costWidth)}`,
  ].join('\n')
}

/** What `tc submit` did, in one line. */
export function submittedText(done: Submitted): string {
  if (done.count === 0) return 'Nothing to submit.'
  const parts = [`Submitted ${done.count} token ${done.count === 1 ? 'entry' : 'entries'}`]
  if (done.cost !== null) parts.push(money(done.cost, done.currency, true))
  if (done.shareOnTickets !== null) parts.push(`${percent(done.shareOnTickets)} on tickets`)
  return parts.join(' · ')
}
