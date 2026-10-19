// The values the Token Controller mod keeps for a session. What tc says is
// mirrored here so the band and the pane draw without calling tc.

/** An org manager's request to submit before the cutoff moves to `cutoff`. */
export type AskedToSubmit = { cutoff: string; askedBy: string | null }

export type Status = {
  isConnected: boolean
  organization: string | null
  currency: string
  today: number
  shareOnFeatures: number | null
  askedToSubmit: AskedToSubmit | null
}

export type Split = { fraction: number | null; tags: Record<string, string> }

export type ReviewRow = {
  number: number
  sessionId: string | null
  startedAt: string | null
  cost: number | null
  summary: string
  isRejected: boolean
  rejectedReason: string | null
  assignedBy: string | null
  label: string | null
  splits: Split[]
}

export type ReportRow = { value: string | null; cost: number; share: number | null }

export type Report = { by: string; currency: string; rows: ReportRow[]; total: number }

export type Rule = { number: number; text: string }

export type Tab = 'review' | 'report' | 'rules'

/** The session facts the mod counts itself; tc reads the rest from the session files. */
export type Facts = { turns: number; durationSeconds: number; subagents: number }

declare module 'claude-code' {
  interface PluginState {
    'token-controller': {
      status: Status | null
      review: ReviewRow[]
      report: Report | null
      reportBy: string
      rules: Rule[]
      tab: Tab
      editing: number | null
      notice: string | null
      problem: string | null
      /** The feature tc split this session on, for the band. */
      feature: string | null
      /** The tags the person set by hand this session, feature included. */
      handTags: Record<string, string>
      summary: string | null
      facts: Facts
    }
  }
}
