// The values the Token Controller mod keeps for a session. What tc says is
// mirrored here so the band and the pane draw without calling tc.

export type Sheet = {
  organization: string | null
  currency: string
  tokenEntries: number
  cost: number
  shareOnTickets: number | null
  dueAt: string | null
  isLate: boolean
}

export type ReviewRow = {
  number: number
  sessionId: string | null
  startedAt: string | null
  cost: number
  tags: Record<string, string>
  by: string
  summary: string
}

export type ReportRow = { value: string; cost: number; share: number | null }

export type Report = { by: string; currency: string; rows: ReportRow[]; total: number }

export type Rule = { number: number; rule: string }

export type Tab = 'review' | 'report' | 'rules'

declare module 'claude-code' {
  interface PluginState {
    'token-controller': {
      sheet: Sheet | null
      review: ReviewRow[]
      report: Report | null
      reportBy: string
      rules: Rule[]
      tab: Tab
      editing: number | null
      notice: string | null
      problem: string | null
      ticket: string | null
      keys: string[]
      summary: string | null
      cost: number | null
    }
  }
}
