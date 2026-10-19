import type { Report, ReportRow, ReviewRow, Status, Submitted } from './tc.ts'

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
 * The headline of the band and the pane: today's cost, the share on features,
 * and this session's feature, such as `$41.87 today · 91% on features · KD-214`.
 */
export function headline(status: Status, feature: string | null): string {
  const parts = [`${money(status.today, status.currency)} today`]
  if (status.shareOnFeatures !== null) {
    parts.push(`${percent(status.shareOnFeatures)} on features`)
  }
  if (feature) parts.push(feature)
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

/** `30 September 2026` for `2026-09-30`; anything else as tc sent it. */
export function dateName(date: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date)
  const month = match === null ? undefined : MONTHS[Number(match[2]) - 1]
  return match === null || month === undefined ? date : `${Number(match[3])} ${month} ${match[1]}`
}

/**
 * The org manager's request to submit before the cutoff moves, such as
 * `Nadia Berger asks you to submit before the cutoff moves to 30 September 2026`.
 */
export function statusNote(status: Status): string | null {
  const asked = status.askedToSubmit
  if (asked === null) return null
  const until = `before the cutoff moves to ${dateName(asked.cutoff)}`
  return asked.askedBy === null
    ? `asked to submit ${until}`
    : `${asked.askedBy} asks you to submit ${until}`
}

function features(row: ReviewRow): string[] {
  return [...new Set(row.splits.flatMap(split => (split.tags.feature ? [split.tags.feature] : [])))]
}

/** The feature tc split this session on, if any: `KD-214`, or `KD-231 +1` when split. */
export function sessionFeature(rows: readonly ReviewRow[], sessionId: string): string | null {
  const row = rows.find(r => r.sessionId === sessionId)
  if (row === undefined) return null
  const [first, ...rest] = features(row)
  if (first === undefined) return null
  return rest.length === 0 ? first : `${first} +${rest.length}`
}

/** `feature KD-214 · team web`, the check list's tag column. */
export function tagLine(tags: Record<string, string>): string {
  const order = ['feature', 'project']
  const names = Object.keys(tags).sort((a, b) => {
    const ia = order.indexOf(a)
    const ib = order.indexOf(b)
    return (ia === -1 ? 9 : ia) - (ib === -1 ? 9 : ib) || a.localeCompare(b)
  })
  return names.map(name => `${name} ${tags[name]}`).join(' · ')
}

/** Where a session sits: its tags, or each split's feature or tags with its fraction. */
export function splitLine(row: ReviewRow): string {
  if (row.splits.length <= 1) return tagLine(row.splits[0]?.tags ?? {}) || 'no project'
  return row.splits
    .map(split => {
      const place = split.tags.feature ?? (tagLine(split.tags) || 'no project')
      return split.fraction === null ? place : `${place} ${percent(split.fraction)}`
    })
    .join(' · ')
}

/** The line under a session: what assigned it and its label, or why it came back. */
export function assignedLine(row: ReviewRow): string {
  if (row.isRejected) return `rejected: ${row.rejectedReason ?? 'see the web app'}`
  const parts: string[] = []
  if (row.assignedBy !== null) parts.push(row.assignedBy)
  if (row.label !== null) parts.push(row.label)
  return parts.join(' · ') || 'no rule matched'
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

/** A row's label: its value, or `(no feature)` for the cost without one. */
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
  const parts = [`Submitted ${done.count} ${done.count === 1 ? 'session' : 'sessions'}`]
  if (done.cost !== null) parts.push(money(done.cost, done.currency, true))
  if (done.shareOnFeatures !== null) parts.push(`${percent(done.shareOnFeatures)} on features`)
  return parts.join(' · ')
}
