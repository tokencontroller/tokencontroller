/**
 * A token entry as the token entries endpoint takes it. Field by field the
 * same as https://docs.tokencontroller.com/reference/token-entry.
 */
export interface TokenEntry {
  /** The product the agent ran on, such as `claude-code` or `support-desk`. */
  agent_harness: string
  /** A label for the agent, such as "review bot". */
  agent_name?: string
  /** The model that did the work. */
  model: string
  /** When the session started, as an ISO 8601 timestamp. */
  started_at: string
  /** When the session ended, as an ISO 8601 timestamp. */
  ended_at: string
  /** Counts by kind. At least one kind is required. */
  tokens: Tokens
  /** The agent's own cost figure. Without it the list value is computed. */
  reported_cost?: number
  /** What the session did, from one line to a short report. */
  summary?: string
  /** The ticket it belongs to. Wins over every rule. */
  ticket?: string
  /** The project it belongs to. */
  project?: string
  /** Tags from the organization's list, one value per tag name. Win over inherited ones. */
  tags?: Record<string, string>
  /** What the agent saw; rules place the token entry with these. */
  signals?: Signals
  /** Joins the token entry to telemetry. */
  session_id?: string
}

/** Token counts by kind. Whole, non-negative numbers. */
export interface Tokens {
  input?: number
  output?: number
  cache_write?: number
  cache_read?: number
}

/**
 * Branch, working directory, repository, pull request number, subagent type,
 * and ticket keys found in prompts, the session title and the pull request
 * title. Only the keys leave the machine, never the prompt.
 */
export interface Signals {
  branch?: string
  working_directory?: string
  repository?: string
  pull_request?: number
  subagent_type?: string
  ticket_keys?: string[]
}

const TOKEN_KINDS = ['input', 'output', 'cache_write', 'cache_read'] as const

/**
 * Every problem with a token entry, in plain words. Empty when the entry is
 * fine to send. The service checks the same and more; this catches mistakes
 * before a request is made.
 */
export function problems(entry: TokenEntry): string[] {
  const found: string[] = []
  for (const field of ['agent_harness', 'model', 'started_at', 'ended_at'] as const) {
    if (typeof entry[field] !== 'string' || entry[field].trim() === '') {
      found.push(`${field} is required`)
    }
  }
  const started = Date.parse(entry.started_at)
  const ended = Date.parse(entry.ended_at)
  if (entry.started_at && Number.isNaN(started)) found.push('started_at is not a timestamp')
  if (entry.ended_at && Number.isNaN(ended)) found.push('ended_at is not a timestamp')
  if (!Number.isNaN(started) && !Number.isNaN(ended) && ended < started) {
    found.push('ended_at is before started_at')
  }
  if (typeof entry.tokens !== 'object' || entry.tokens === null) {
    found.push('tokens is required')
  } else {
    const kinds = Object.keys(entry.tokens)
    for (const kind of kinds) {
      if (!(TOKEN_KINDS as readonly string[]).includes(kind)) {
        found.push(`tokens.${kind} is not a token kind (${TOKEN_KINDS.join(', ')})`)
      }
    }
    for (const kind of TOKEN_KINDS) {
      const count = entry.tokens[kind]
      if (count !== undefined && !(Number.isSafeInteger(count) && count >= 0)) {
        found.push(`tokens.${kind} must be a whole number, 0 or more`)
      }
    }
    if (!TOKEN_KINDS.some(kind => entry.tokens[kind] !== undefined)) {
      found.push('tokens needs at least one kind')
    }
  }
  if (
    entry.reported_cost !== undefined &&
    !(Number.isFinite(entry.reported_cost) && entry.reported_cost >= 0)
  ) {
    found.push('reported_cost must be a number, 0 or more')
  }
  if (entry.tags !== undefined) {
    for (const [name, value] of Object.entries(entry.tags)) {
      if (typeof value !== 'string') found.push(`tags.${name} must be one text value`)
    }
  }
  const pr = entry.signals?.pull_request
  if (pr !== undefined && !(Number.isSafeInteger(pr) && pr > 0)) {
    found.push('signals.pull_request must be a pull request number')
  }
  return found
}
