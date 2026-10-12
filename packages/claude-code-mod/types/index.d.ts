// The values the Token Controller mod keeps for a session. What tc says is
// mirrored here so the band and the pane draw without calling tc.

export type Sheet = {
  isConnected: boolean
  organization: string | null
  currency: string
  today: number
  shareOnTickets: number | null
  isLate: boolean
}

export type Split = { share: number | null; tags: Record<string, string> }

export type ReviewRow = {
  number: number
  sessionId: string | null
  startedAt: string | null
  cost: number | null
  summary: string
  isRejected: boolean
  rejectedReason: string | null
  placedBy: string | null
  candidates: string[]
  splits: Split[]
}

export type ReportRow = { value: string | null; cost: number; share: number | null }

export type Report = { by: string; currency: string; rows: ReportRow[]; total: number }

export type Rule = { number: number; text: string }

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
      /** Where tc placed this session, for the band. */
      ticket: string | null
      /** The tags the person set by hand this session, ticket included. */
      handTags: Record<string, string>
      summary: string | null
    }
  }
}
