import type { Report, ReviewRow, Sheet } from './tc.ts'

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
 * The headline of the band and the pane, as the docs show it:
 * `$212 spent · 91% on tickets · KD-214`.
 */
export function headline(sheet: Sheet, ticket: string | null): string {
  const parts = [`${money(sheet.cost, sheet.currency)} spent`]
  if (sheet.shareOnTickets !== null) parts.push(`${percent(sheet.shareOnTickets)} on tickets`)
  if (ticket) parts.push(ticket)
  return parts.join(' · ')
}

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

/** `token sheet late`, `token sheet due Fri`, or nothing while it is far off. */
export function sheetNote(sheet: Sheet, now: number): string | null {
  if (sheet.isLate) return 'token sheet late'
  if (sheet.dueAt === null) return null
  const due = Date.parse(sheet.dueAt)
  if (Number.isNaN(due)) return null
  const days = (due - now) / 86_400_000
  if (days < 0) return 'token sheet late'
  if (days <= 3) return `token sheet due ${DAYS[new Date(due).getDay()]}`
  return null
}

/** The ticket tc placed this session on, if any. */
export function sessionTicket(rows: readonly ReviewRow[], sessionId: string): string | null {
  return rows.find(row => row.sessionId === sessionId)?.tags.ticket ?? null
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

/** A report as a Markdown table, for the model and for `/tc report`. */
export function reportTable(report: Report): string {
  if (report.rows.length === 0) return `No cost by ${report.by} yet.`
  const lines = [
    `| ${report.by} | cost | share |`,
    '| --- | ---: | ---: |',
    ...report.rows.map(
      row =>
        `| ${row.value} | ${money(row.cost, report.currency, true)} | ${row.share === null ? '' : percent(row.share)} |`,
    ),
    `| **total** | **${money(report.total, report.currency, true)}** | |`,
  ]
  return lines.join('\n')
}

/** The same report as aligned text with bars, for a command's output row. */
export function reportText(report: Report, width = 72): string {
  if (report.rows.length === 0) return `No cost by ${report.by} yet.`
  const label = Math.min(24, Math.max(report.by.length, ...report.rows.map(r => r.value.length)))
  const costs = report.rows.map(row => money(row.cost, report.currency, true))
  const costWidth = Math.max(...costs.map(c => c.length))
  const barWidth = Math.min(40, Math.max(8, width - label - costWidth - 10))
  const max = Math.max(...report.rows.map(row => row.cost))
  const lines = report.rows.map((row, i) => {
    const share = row.share === null ? '' : percent(row.share)
    return `${row.value.slice(0, label).padEnd(label)}  ${(costs[i] ?? '').padStart(costWidth)}  ${share.padStart(4)}  ${bar(row.cost, max, barWidth)}`
  })
  return [
    `Cost by ${report.by}`,
    '',
    ...lines,
    `${'total'.padEnd(label)}  ${money(report.total, report.currency, true).padStart(costWidth)}`,
  ].join('\n')
}
