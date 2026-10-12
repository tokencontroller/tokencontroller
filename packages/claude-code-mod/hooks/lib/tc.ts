/**
 * The parts of `tc --json` output the mod reads. tc owns local state, rules
 * and submission (ADR 0010); the mod only shows what tc says and passes on
 * what the person does. Every reader here is lenient: a field tc leaves out
 * reads as unknown, never as a crash.
 */

export interface Sheet {
  organization: string | null
  currency: string
  tokenEntries: number
  cost: number
  shareOnTickets: number | null
  dueAt: string | null
  isLate: boolean
}

export interface ReviewRow {
  number: number
  sessionId: string | null
  startedAt: string | null
  cost: number
  tags: Record<string, string>
  by: string
  summary: string
}

export interface ReportRow {
  value: string
  cost: number
  share: number | null
}

export interface Report {
  by: string
  currency: string
  rows: ReportRow[]
  total: number
}

export interface Rule {
  number: number
  rule: string
}

type Json = Record<string, unknown>

function object(value: unknown): Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Json) : {}
}

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

function tags(value: unknown): Record<string, string> {
  const out: Record<string, string> = {}
  for (const [name, v] of Object.entries(object(value))) {
    if (typeof v === 'string') out[name] = v
  }
  return out
}

function list(value: unknown, key: string): unknown[] {
  if (Array.isArray(value)) return value
  const inner = object(value)[key]
  return Array.isArray(inner) ? inner : []
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
  const sheet = object(top.token_sheet)
  return {
    organization: str(top.organization),
    currency: str(top.currency) ?? 'USD',
    tokenEntries: num(sheet.token_entries) ?? 0,
    cost: num(sheet.cost) ?? 0,
    shareOnTickets: num(sheet.share_on_tickets),
    dueAt: str(sheet.due_at),
    isLate: sheet.is_late === true,
  }
}

export function readReview(value: unknown): ReviewRow[] {
  return list(value, 'token_entries').flatMap(item => {
    const row = object(item)
    const number = num(row.number)
    if (number === null) return []
    return [
      {
        number,
        sessionId: str(row.session_id),
        startedAt: str(row.started_at),
        cost: num(row.cost) ?? 0,
        tags: tags(row.tags),
        by: str(row.by) ?? '',
        summary: str(row.summary) ?? '',
      },
    ]
  })
}

export function readReport(value: unknown, by: string): Report {
  const top = object(value)
  const rows = list(value, 'rows').flatMap(item => {
    const row = object(item)
    const label = str(row.value) ?? str(row.name)
    const cost = num(row.cost)
    return label === null || cost === null ? [] : [{ value: label, cost, share: num(row.share) }]
  })
  return {
    by: str(top.by) ?? by,
    currency: str(top.currency) ?? 'USD',
    rows,
    total: num(top.total) ?? rows.reduce((sum, row) => sum + row.cost, 0),
  }
}

export function readRules(value: unknown): Rule[] {
  return list(value, 'rules').flatMap(item => {
    const row = object(item)
    const number = num(row.number)
    const rule = str(row.rule) ?? str(row.text)
    return number === null || rule === null ? [] : [{ number, rule }]
  })
}

/** The first line tc wrote on stderr, or a plain fallback: what the person reads. */
export function reason(stderr: string, stdout: string, fallback: string): string {
  const line = (stderr.trim() || stdout.trim()).split('\n')[0]?.trim()
  return line || fallback
}
