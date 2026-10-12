/**
 * The parts of `tc --json` output the mod reads. tc owns local state, rules
 * and submission (ADR 0010); the mod only shows what tc says and passes on
 * what the person does. Every reader here is lenient: a field tc leaves out
 * reads as unknown, never as a crash.
 */

/** `tc status --json`: today's cost and the token sheet. */
export interface Sheet {
  isConnected: boolean
  organization: string | null
  currency: string
  /** Today's list value. */
  today: number
  /** 0 to 1, or null while there is nothing to share. */
  shareOnTickets: number | null
  isLate: boolean
}

export interface Split {
  /** 0 to 1, or null when tc does not say. */
  share: number | null
  tags: Record<string, string>
}

/** One token entry of `tc review --json`. */
export interface ReviewRow {
  number: number
  sessionId: string | null
  startedAt: string | null
  /** The list value, or null while the model has no price. */
  cost: number | null
  summary: string
  isRejected: boolean
  rejectedReason: string | null
  placedBy: string | null
  candidates: string[]
  splits: Split[]
}

export interface ReportRow {
  /** The tag value, or null for the cost that has none. */
  value: string | null
  cost: number
  /** 0 to 1 of the total. */
  share: number | null
}

export interface Report {
  by: string
  currency: string
  rows: ReportRow[]
  total: number
}

export interface Rule {
  /** Its place in the list, as `tc rules remove N` takes it. */
  number: number
  text: string
}

/** `tc submit --json`. */
export interface Submitted {
  count: number
  cost: number | null
  currency: string
  shareOnTickets: number | null
}

type Json = Record<string, unknown>

function object(value: unknown): Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Json) : {}
}

/** A number, or a decimal string as tc sends money. */
function num(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string' && value.trim() !== '' && Number.isFinite(Number(value))) {
    return Number(value)
  }
  return null
}

function str(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null
}

function fraction(percent: unknown): number | null {
  const n = num(percent)
  return n === null ? null : n / 100
}

function tags(value: unknown): Record<string, string> {
  const out: Record<string, string> = {}
  for (const [name, v] of Object.entries(object(value))) {
    if (typeof v === 'string') out[name] = v
  }
  return out
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

/** A list, bare or under `key`. */
function list(value: unknown, key: string): unknown[] {
  return Array.isArray(value) ? value : array(object(value)[key])
}

export function parseJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

export function readSheet(value: unknown): Sheet {
  const top = object(value)
  const today = object(top.today)
  return {
    isConnected: top.connected === true,
    organization: str(top.organization),
    currency: str(today.currency) ?? 'USD',
    today: num(today.list_value) ?? 0,
    shareOnTickets: fraction(today.share_on_tickets_percent),
    isLate: object(top.token_sheet).late === true,
  }
}

export function readEntry(value: unknown): ReviewRow | null {
  const row = object(value)
  const number = num(row.number)
  if (number === null) return null
  return {
    number,
    sessionId: str(row.session_id),
    startedAt: str(row.started_at),
    cost: num(row.list_value),
    summary: str(row.summary) ?? '',
    isRejected: row.status === 'rejected',
    rejectedReason: str(row.rejected_reason),
    placedBy: str(row.placed_by),
    candidates: array(row.candidates).flatMap(key => (typeof key === 'string' ? [key] : [])),
    splits: array(row.splits).map(item => {
      const split = object(item)
      return { share: fraction(split.share_percent), tags: tags(split.tags) }
    }),
  }
}

export function readReview(value: unknown): ReviewRow[] {
  return list(value, 'token_entries').flatMap(item => {
    const row = readEntry(item)
    return row === null ? [] : [row]
  })
}

export function readReport(value: unknown, by: string): Report {
  const top = object(value)
  const rows = list(value, 'rows').flatMap(item => {
    const row = object(item)
    const cost = num(row.list_value)
    return cost === null ? [] : [{ value: str(row.value), cost }]
  })
  const total = num(top.total) ?? rows.reduce((sum, row) => sum + row.cost, 0)
  return {
    by: str(top.by) ?? by,
    currency: str(top.currency) ?? 'USD',
    rows: rows.map(row => ({ ...row, share: total > 0 ? row.cost / total : null })),
    total,
  }
}

export function readRules(value: unknown): Rule[] {
  return list(value, 'rules').flatMap((item, index) => {
    const row = object(item)
    const text = str(row.text) ?? str(row.name)
    return text === null ? [] : [{ number: num(row.number) ?? index + 1, text }]
  })
}

export function readSubmitted(value: unknown): Submitted {
  const top = object(value)
  return {
    count: num(top.submitted) ?? 0,
    cost: num(top.list_value),
    currency: str(top.currency) ?? 'USD',
    shareOnTickets: fraction(top.share_on_tickets_percent),
  }
}

/** The first line tc wrote on stderr, or a plain fallback: what the person reads. */
export function reason(stderr: string, stdout: string, fallback: string): string {
  const line = (stderr.trim() || stdout.trim()).split('\n')[0]?.trim()
  return line || fallback
}
