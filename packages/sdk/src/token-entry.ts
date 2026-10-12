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
  /**
   * The agent's own cost figure in US dollars, as a number or a decimal
   * string such as `"0.031"`. Without it the list value is computed.
   */
  reported_cost?: Amount
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
  /** Joins the token entry to telemetry. Sending an entry again with the same one updates it. */
  session_id?: string
  /**
   * The session's model calls. With them, a session telemetry also reported
   * is counted exactly once, call by call. Their tokens add up to `tokens`.
   */
  messages?: Message[]
  /** How the entry is split: by `message_ids` (segments) or by `share_percent` (by hand). */
  splits?: Split[]
  /** Your own ID for the entry, such as `claude-code:<session id>`. Sending it again replaces the earlier one. */
  local_id?: string
  /** What placed it: a rule's name, `hand`, `classifier` or `none`. */
  placed_by?: string
}

/** US dollars, as a number or a decimal string such as `"0.031"`. */
export type Amount = number | string

/** One model call of a session. */
export interface Message {
  /** The agent harness's ID for the call, unique within the session. */
  message_id: string
  /** The provider's request ID, when known. */
  request_id?: string
  /** The model of this call, when it differs from the entry's. */
  model?: string
  /** When the call was made, as an ISO 8601 timestamp. */
  at: string
  tokens: Tokens
  /** The agent's own cost figure for this call. */
  reported_cost?: Amount
}

/** One part of a token entry with its own tags: a segment by messages, or a share by hand. */
export interface Split {
  message_ids?: string[]
  share_percent?: number
  tags?: Record<string, string>
  signals?: Signals
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
  if (entry.reported_cost !== undefined && !isAmount(entry.reported_cost)) {
    found.push('reported_cost must be an amount, 0 or more')
  }
  found.push(...tagProblems('tags', entry.tags))
  for (const field of ['ticket', 'project'] as const) {
    const tag = entry.tags?.[field]
    if (entry[field] !== undefined && tag !== undefined && entry[field] !== tag) {
      found.push(`${field} and tags.${field} differ: send one`)
    }
  }
  const pr = entry.signals?.pull_request
  if (pr !== undefined && !(Number.isSafeInteger(pr) && pr > 0)) {
    found.push('signals.pull_request must be a pull request number')
  }
  found.push(...messageProblems(entry), ...splitProblems(entry))
  return found
}

const DECIMAL = /^\d+(\.\d+)?$/

function isAmount(amount: Amount): boolean {
  return typeof amount === 'string'
    ? DECIMAL.test(amount.trim())
    : Number.isFinite(amount) && amount >= 0
}

const TAG_NAME = /^[a-z][a-z0-9_-]*$/

function tagProblems(where: string, tags: Record<string, string> | undefined): string[] {
  const found: string[] = []
  for (const [name, value] of Object.entries(tags ?? {})) {
    if (!TAG_NAME.test(name)) {
      found.push(`${where}.${name}: a tag name is lowercase letters, digits, _ or -`)
    } else if (name === 'person') {
      found.push(`${where}.person is set from the token that sends the entry`)
    }
    if (typeof value !== 'string' || value.trim() === '') {
      found.push(`${where}.${name} must be one text value`)
    }
  }
  return found
}

function messageProblems(entry: TokenEntry): string[] {
  const messages = entry.messages ?? []
  if (messages.length === 0) return []
  const found: string[] = []
  if (entry.session_id === undefined) found.push('messages need the session_id they belong to')
  const seen = new Set<string>()
  const sum = { input: 0, output: 0, cache_write: 0, cache_read: 0 }
  for (const message of messages) {
    if (seen.has(message.message_id)) found.push(`message ${message.message_id} appears twice`)
    seen.add(message.message_id)
    if (Number.isNaN(Date.parse(message.at))) {
      found.push(`message ${message.message_id}: at is not a timestamp`)
    }
    if (message.reported_cost !== undefined && !isAmount(message.reported_cost)) {
      found.push(`message ${message.message_id}: reported_cost must be an amount, 0 or more`)
    }
    for (const kind of TOKEN_KINDS) sum[kind] += message.tokens?.[kind] ?? 0
  }
  if (TOKEN_KINDS.some(kind => sum[kind] !== (entry.tokens?.[kind] ?? 0))) {
    found.push("tokens must equal the sum of the messages' tokens")
  }
  return found
}

function splitProblems(entry: TokenEntry): string[] {
  const splits = entry.splits ?? []
  if (splits.length === 0) return []
  const found = splits.flatMap((split, i) => tagProblems(`splits[${i}].tags`, split.tags))
  const byMessage = splits.every(
    s => (s.message_ids?.length ?? 0) > 0 && s.share_percent === undefined,
  )
  const byShare = splits.every(
    s => (s.message_ids?.length ?? 0) === 0 && s.share_percent !== undefined,
  )
  if (byMessage) {
    const ids = splits.flatMap(split => split.message_ids ?? [])
    const messages = new Set((entry.messages ?? []).map(message => message.message_id))
    if (
      new Set(ids).size !== ids.length ||
      ids.length !== messages.size ||
      ids.some(id => !messages.has(id))
    ) {
      found.push('splits by message must hold every message of the entry exactly once')
    }
  } else if (byShare) {
    const shares = splits.map(split => split.share_percent ?? 0)
    if (shares.some(share => !Number.isInteger(share) || share <= 0)) {
      found.push('a split share is a whole percent above 0')
    } else if (shares.reduce((a, b) => a + b, 0) !== 100) {
      found.push('split shares must add up to 100 percent')
    }
  } else {
    found.push('splits are either all by message_ids or all by share_percent')
  }
  return found
}
