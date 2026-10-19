/**
 * A token entry as the token entries endpoint takes it: one message, or one
 * session with its messages. Field by field the same as
 * https://docs.tokencontroller.com/reference/token-entry.
 */
export interface TokenEntry {
  /** The product the agent ran on, such as `claude-code` or `support-desk`. */
  agent_harness: string
  /** A label for the agent, such as "review bot". */
  agent_name?: string
  /**
   * Who charges for the model calls: `anthropic`, `openai`, `google`,
   * `aws_bedrock`, `gcp_vertex`, `azure_foundry`, `github`, `cursor` or
   * `other`. Without it, it follows from the model's name.
   */
  provider?: string
  /** The model that did the work. */
  model: string
  /** When the session started, as an ISO 8601 timestamp. */
  started_at: string
  /** When the session ended, as an ISO 8601 timestamp. */
  ended_at: string
  /** Counts by kind. All four kinds are required; a missing kind is a count error. */
  tokens: Tokens
  /**
   * The agent's own cost figure in US dollars, as a number or a decimal
   * string such as `"0.031"`. Stored as a fact about the message; the cost
   * shown is counts × the price table.
   */
  reported_cost?: Amount
  /** What the session did, from one line to a short report. With the tag values, the only text stored. */
  summary?: string
  /** Session facts: turns, tool calls, retries, duration, subagents, files touched, the result. */
  facts?: Facts
  /** The feature it belongs to, by key such as `KD-214`. Wins over every rule. */
  feature?: string
  /** The project it belongs to, by key. Wins over every rule. */
  project?: string
  /** Any other tags, such as `{ purpose: 'code-review' }`. One value per tag name. Win over rules. */
  tags?: Record<string, string>
  /** Facts about the session that rules read. */
  signals?: Signals
  /** The agent harness's id for the session. A message seen again with the same session id and message id is the same record. */
  session_id?: string
  /**
   * The session's model calls. With them, a session that telemetry also
   * reported is counted exactly once, call by call. Their tokens add up to `tokens`.
   */
  messages?: Message[]
  /** How the session is split. Without it the session is one split, or rules split it. */
  splits?: Split[]
  /** Your own id for the entry, such as `claude-code:<session id>`. Sending it again adds nothing. */
  local_id?: string
}

/** US dollars, as a number or a decimal string such as `"0.031"`. */
export type Amount = number | string

/** A fraction of 1, as a number or a decimal string such as `"0.6"`. */
export type Fraction = number | string

/** Session facts. All optional. Never prompt text. */
export interface Facts {
  turns?: number
  tool_calls?: number
  retries?: number
  duration_seconds?: number
  subagents?: number
  files_touched?: number
  /** A URL such as the pull request. */
  result?: string
}

/** One model call of a session. */
export interface Message {
  /** The agent harness's id for the call, unique within the session. */
  message_id: string
  /** When the call was made, as an ISO 8601 timestamp. */
  at: string
  /** Counts by kind. All four kinds are required. */
  tokens: Tokens
  /** The model of this call, when it differs from the session's. */
  model?: string
  /** The agent's own cost figure for this call, in US dollars. A fact, never the figure. */
  reported_cost?: Amount
}

/**
 * One split of a session: its tags and a `fraction` of the session, or the
 * `message_ids` it covers exactly. Fractions add up to 1.
 */
export interface Split {
  fraction?: Fraction
  message_ids?: string[]
  tags?: Record<string, string>
}

/** Token counts by kind. Whole, non-negative numbers; all four required. */
export interface Tokens {
  input: number
  output: number
  cache_read: number
  cache_write: number
}

/**
 * Signals: facts about the session that rules read. Branch, working
 * directory, repository, pull request number and title, subagent type, the
 * keys found in prompts and titles, and values from the repository's
 * `.tokencontroller.toml`. Only the keys leave the machine, never the prompt.
 */
export interface Signals {
  branch?: string
  working_directory?: string
  repository?: string
  pull_request?: number
  pull_request_title?: string
  subagent_type?: string
  /** Keys found in prompts, the session title and the pull request title. */
  keys?: string[]
  /** Values from the repository's `.tokencontroller.toml`, as `name: value`. */
  config?: Record<string, string>
}

const TOKEN_KINDS = ['input', 'output', 'cache_read', 'cache_write'] as const

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
  found.push(...tokenProblems('tokens', entry.tokens))
  if (entry.reported_cost !== undefined && !isAmount(entry.reported_cost)) {
    found.push('reported_cost must be an amount, 0 or more')
  }
  found.push(...factProblems(entry.facts))
  found.push(...tagProblems('tags', entry.tags))
  for (const field of ['feature', 'project'] as const) {
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

/** All four kinds, each a whole number, 0 or more; no other kind. */
function tokenProblems(where: string, tokens: Tokens | undefined): string[] {
  if (typeof tokens !== 'object' || tokens === null) return [`${where} is required`]
  const found: string[] = []
  for (const kind of Object.keys(tokens)) {
    if (!(TOKEN_KINDS as readonly string[]).includes(kind)) {
      found.push(`${where}.${kind} is not a token kind (${TOKEN_KINDS.join(', ')})`)
    }
  }
  for (const kind of TOKEN_KINDS) {
    const count = tokens[kind]
    if (count === undefined) found.push(`${where}.${kind} is required: all four kinds are sent`)
    else if (!(Number.isSafeInteger(count) && count >= 0)) {
      found.push(`${where}.${kind} must be a whole number, 0 or more`)
    }
  }
  return found
}

const DECIMAL = /^\d+(\.\d+)?$/

function isAmount(amount: Amount): boolean {
  return typeof amount === 'string'
    ? DECIMAL.test(amount.trim())
    : Number.isFinite(amount) && amount >= 0
}

const COUNT_FACTS = [
  'turns',
  'tool_calls',
  'retries',
  'duration_seconds',
  'subagents',
  'files_touched',
] as const

function factProblems(facts: Facts | undefined): string[] {
  if (facts === undefined) return []
  const found: string[] = []
  for (const name of COUNT_FACTS) {
    const value = facts[name]
    if (value !== undefined && !(Number.isFinite(value) && value >= 0)) {
      found.push(`facts.${name} must be a number, 0 or more`)
    }
  }
  if (facts.result !== undefined && (typeof facts.result !== 'string' || facts.result === '')) {
    found.push('facts.result must be a URL or another pointer, as text')
  }
  return found
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
  const sum = { input: 0, output: 0, cache_read: 0, cache_write: 0 }
  for (const message of messages) {
    if (seen.has(message.message_id)) found.push(`message ${message.message_id} appears twice`)
    seen.add(message.message_id)
    if (Number.isNaN(Date.parse(message.at))) {
      found.push(`message ${message.message_id}: at is not a timestamp`)
    }
    found.push(...tokenProblems(`message ${message.message_id}: tokens`, message.tokens))
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

/** A fraction as a number: `0.6` or `"0.6"`; null when it is none. */
function fractionOf(fraction: Fraction | undefined): number | null {
  if (typeof fraction === 'number') return Number.isFinite(fraction) ? fraction : null
  if (typeof fraction === 'string' && DECIMAL.test(fraction.trim())) return Number(fraction)
  return null
}

function splitProblems(entry: TokenEntry): string[] {
  const splits = entry.splits ?? []
  if (splits.length === 0) return []
  const found = splits.flatMap((split, i) => tagProblems(`splits[${i}].tags`, split.tags))
  const byMessage = splits.every(s => (s.message_ids?.length ?? 0) > 0 && s.fraction === undefined)
  const byFraction = splits.every(
    s => (s.message_ids?.length ?? 0) === 0 && s.fraction !== undefined,
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
  } else if (byFraction) {
    const fractions = splits.map(split => fractionOf(split.fraction))
    if (fractions.some(fraction => fraction === null || fraction <= 0 || fraction > 1)) {
      found.push('a split fraction is a number above 0 and up to 1, such as 0.6 or "0.6"')
    } else {
      const sum = fractions.reduce<number>((a, b) => a + (b ?? 0), 0)
      if (Math.abs(sum - 1) > 1e-6) found.push('split fractions must add up to 1')
    }
  } else {
    found.push('splits are either all by message_ids or all by fraction')
  }
  return found
}
