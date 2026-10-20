import { problems, type TokenEntry } from './token-entry.ts'

/** Where the hosted service runs. Self-hosted instances pass their own `url`. */
export const HOSTED_URL = 'https://app.tokencontroller.com'

export interface ClientOptions {
  /** A service token (`tc_svc_…`). Its creator is the person of every message it reports. */
  token: string
  /** The service's address. Defaults to the hosted service. */
  url?: string
  /** How often a failed request is tried again, for entries with a `session_id` or `local_id`. Defaults to 2. */
  retries?: number
  /** How long one request may take, in milliseconds. Defaults to 30 seconds. */
  timeoutMs?: number
  /** A `fetch` to use instead of the global one, for tests and proxies. */
  fetch?: typeof globalThis.fetch
}

/** What the service answers for a token entry it stored (docs: Token entries endpoint > The answer). */
export interface StoredEntry {
  /** The session the messages were counted under: the entry's `session_id`, else its `local_id`, else one the service made. */
  session_id: string
  /** How many messages the entry stands for. */
  messages: number
  /** The cost from the price table as of now, as a decimal string, or null while the model has no price or counts are missing. */
  cost: string | null
  /** The currency of `cost`, such as `USD`. */
  currency: string
  /** The label of the entry's largest part: `direct`, `project_overhead` or `organization_overhead`. */
  label: string
  /** Tags the entry named that are no value on record yet, as `name: key`. They count as sent; someone with assign creates the value. */
  unknown_tags: string[]
  [field: string]: unknown
}

/** A token entry the client or the service refused, or a request that failed. */
export class TokenControllerError extends Error {
  override readonly name = 'TokenControllerError'
  /** The HTTP status, or undefined when no answer came. */
  readonly status: number | undefined
  /** The service's answer, as sent. */
  readonly body: unknown

  constructor(message: string, status?: number, body?: unknown) {
    super(message)
    this.status = status
    this.body = body
  }
}

/**
 * A client for the Token Controller API.
 *
 * @example
 * const tc = new TokenController({ token: process.env.TOKEN_CONTROLLER_TOKEN })
 * await tc.tokenEntries.create({ agent_harness: 'support-desk', model: 'claude-sonnet-5', ... })
 */
export class TokenController {
  readonly tokenEntries: { create: (entry: TokenEntry) => Promise<StoredEntry> }
  readonly #token: string
  readonly #url: string
  readonly #retries: number
  readonly #timeoutMs: number
  readonly #fetch: typeof globalThis.fetch

  constructor(options: ClientOptions) {
    if (typeof options.token !== 'string' || !options.token.startsWith('tc_svc_')) {
      throw new TokenControllerError(
        'A service token starting with tc_svc_ is needed. Create one in the web app under Settings.',
      )
    }
    this.#token = options.token
    this.#url = (options.url ?? HOSTED_URL).replace(/\/+$/, '')
    this.#retries = options.retries ?? 2
    this.#timeoutMs = options.timeoutMs ?? 30_000
    this.#fetch = options.fetch ?? globalThis.fetch
    this.tokenEntries = { create: entry => this.#createTokenEntry(entry) }
  }

  async #createTokenEntry(entry: TokenEntry): Promise<StoredEntry> {
    const found = problems(entry)
    if (found.length > 0) {
      throw new TokenControllerError(`The token entry cannot be sent: ${found.join('; ')}.`)
    }
    // Without a session ID or local ID the service cannot tell a retry from
    // a second session, so such an entry is sent once.
    const isRepeatable = entry.session_id !== undefined || entry.local_id !== undefined
    const tries = isRepeatable ? this.#retries + 1 : 1
    let last: TokenControllerError | undefined
    for (let attempt = 1; attempt <= tries; attempt += 1) {
      try {
        return await this.#post('/api/v1/token-entries', entry)
      } catch (error) {
        if (!(error instanceof TokenControllerError) || !isWorthRetrying(error)) throw error
        last = error
        if (attempt < tries) await sleep(250 * 2 ** (attempt - 1))
      }
    }
    throw last ?? new TokenControllerError('The request failed.')
  }

  async #post(path: string, body: unknown): Promise<StoredEntry> {
    let response: Response
    try {
      response = await this.#fetch(`${this.#url}${path}`, {
        method: 'POST',
        headers: {
          authorization: `Bearer ${this.#token}`,
          'content-type': 'application/json',
          accept: 'application/json',
        },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(this.#timeoutMs),
      })
    } catch (error) {
      const why = error instanceof Error ? error.message : String(error)
      throw new TokenControllerError(`Could not reach ${this.#url}: ${why}`)
    }
    const text = await response.text()
    const answer = parseJson(text)
    if (!response.ok) {
      throw new TokenControllerError(
        `${this.#url} answered ${response.status}: ${messageOf(answer) ?? (text.slice(0, 200) || response.statusText)}`,
        response.status,
        answer,
      )
    }
    if (
      typeof answer !== 'object' ||
      answer === null ||
      typeof (answer as StoredEntry).session_id !== 'string'
    ) {
      throw new TokenControllerError(
        `${this.#url} answered without a stored token entry`,
        response.status,
        answer,
      )
    }
    return answer as StoredEntry
  }
}

function isWorthRetrying(error: TokenControllerError): boolean {
  return error.status === undefined || error.status === 429 || error.status >= 500
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return text
  }
}

function messageOf(answer: unknown): string | undefined {
  if (typeof answer !== 'object' || answer === null) return undefined
  const { message, error } = answer as { message?: unknown; error?: unknown }
  if (typeof message === 'string') return message
  if (typeof error === 'string') return error
  return undefined
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}
