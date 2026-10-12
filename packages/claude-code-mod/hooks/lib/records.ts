/**
 * What the mod hands to `tc record`: one JSON object per line. tc builds the
 * session's draft token entry, its segments and its tags from these, and runs
 * the rules (ADR 0010). The mod adds only what Claude Code alone sees.
 */

/** Claude Code's own usage for one turn, as `turn.complete` reports it. */
export interface TurnUsage {
  model: string
  input_tokens: number
  output_tokens: number
  cache_read_input_tokens: number
  cache_creation_input_tokens: number
}

export interface TurnRecord {
  kind: 'turn'
  session_id: string
  turn_id: string
  /** The subagent the turn ran in; absent on the main conversation. */
  agent_id?: string
  at: string
  model: string
  tokens: { input: number; output: number; cache_write: number; cache_read: number }
  /** US dollars the turn added to the session's cost, as Claude Code counts it. */
  reported_cost: number | null
  cwd: string
  branch: string | null
  repository: string | null
  ticket_keys: string[]
}

export interface SummaryRecord {
  kind: 'summary'
  session_id: string
  summary: string
}

/** Tags the person set by hand for the whole session (`/tc ticket`, `/tc tag`). */
export interface TagsRecord {
  kind: 'tags'
  session_id: string
  tags: Record<string, string>
}

export type TcRecord = TurnRecord | SummaryRecord | TagsRecord

export interface TurnFacts {
  sessionId: string
  turnId: string
  agentId: string | undefined
  at: number
  usage: TurnUsage
  costBefore: number | undefined
  costAfter: number | undefined
  cwd: string
  branch: string | null
  repository: string | null
  ticketKeys: readonly string[]
}

export function turnRecord(facts: TurnFacts): TurnRecord {
  const { usage } = facts
  const cost =
    facts.costAfter === undefined
      ? null
      : Math.max(0, round6(facts.costAfter - (facts.costBefore ?? 0)))
  return {
    kind: 'turn',
    session_id: facts.sessionId,
    turn_id: facts.turnId,
    ...(facts.agentId === undefined ? {} : { agent_id: facts.agentId }),
    at: new Date(facts.at).toISOString(),
    model: usage.model,
    tokens: {
      input: usage.input_tokens,
      output: usage.output_tokens,
      cache_write: usage.cache_creation_input_tokens,
      cache_read: usage.cache_read_input_tokens,
    },
    reported_cost: cost,
    cwd: facts.cwd,
    branch: facts.branch,
    repository: facts.repository,
    ticket_keys: [...new Set(facts.ticketKeys)],
  }
}

function round6(n: number): number {
  return Math.round(n * 1e6) / 1e6
}

/**
 * Ticket key candidates in a prompt: Jira-style keys (`KD-214`) and issue
 * numbers with their repository (`storefront#12`). Only these keys leave the
 * prompt; tc keeps the ones that match a project's ticket key pattern.
 */
export function ticketKeyCandidates(text: string): string[] {
  const found = new Set<string>()
  for (const match of text.matchAll(/\b([A-Z][A-Z0-9]{1,9}-\d{1,7})\b/g)) {
    if (match[1] !== undefined) found.add(match[1])
  }
  for (const match of text.matchAll(/\b([A-Za-z0-9_.-]+#\d{1,7})\b/g)) {
    if (match[1] !== undefined) found.add(match[1])
  }
  return [...found]
}

/** `owner/name` from an `origin` URL in any of git's spellings, or null. */
export function repositoryName(remote: string | null | undefined): string | null {
  if (!remote) return null
  const match = /[:/]([^/:]+\/[^/]+?)(?:\.git)?\/?$/.exec(remote.trim())
  return match?.[1] ?? null
}

/** The most lines kept for one session while tc cannot take them. */
export const OUTBOX_LIMIT = 5000

/** Adds lines to an outbox, dropping the oldest turns past the limit. */
export function addToOutbox(outbox: readonly string[], ...records: TcRecord[]): string[] {
  const lines = [...outbox, ...records.map(record => JSON.stringify(record))]
  return lines.length > OUTBOX_LIMIT ? lines.slice(lines.length - OUTBOX_LIMIT) : lines
}

/**
 * Reads `/tc` arguments: `ticket KD-214`, `tag purpose=spike`, `report team`,
 * `submit`, or nothing (opens the pane).
 */
export type Command =
  | { kind: 'open' }
  | { kind: 'tags'; tags: Record<string, string> }
  | { kind: 'report'; by: string }
  | { kind: 'submit' }
  | { kind: 'help'; problem?: string }

export function parseCommand(args: string): Command {
  const words = args.trim().split(/\s+/).filter(Boolean)
  const [verb, ...rest] = words
  switch (verb) {
    case undefined:
      return { kind: 'open' }
    case 'ticket':
      if (rest.length !== 1 || rest[0] === undefined) {
        return { kind: 'help', problem: 'Name one ticket, such as /tc ticket KD-214.' }
      }
      return { kind: 'tags', tags: { ticket: rest[0] } }
    case 'tag': {
      if (rest.length === 0) {
        return { kind: 'help', problem: 'Name a tag, such as /tc tag purpose=spike.' }
      }
      const tags: Record<string, string> = {}
      for (const pair of rest) {
        const at = pair.indexOf('=')
        if (at <= 0 || at === pair.length - 1) {
          return { kind: 'help', problem: `"${pair}" is not name=value.` }
        }
        tags[pair.slice(0, at)] = pair.slice(at + 1)
      }
      return { kind: 'tags', tags }
    }
    case 'report':
      return { kind: 'report', by: rest[0] ?? 'ticket' }
    case 'submit':
      return { kind: 'submit' }
    default:
      return { kind: 'help', problem: `/tc has no "${verb}".` }
  }
}

export const HELP = [
  '/tc ticket KD-214     set this session’s ticket',
  '/tc tag purpose=spike set any other tag',
  '/tc report [by]       a report table, by ticket, project, any tag or day',
  '/tc submit            submit your token sheet',
  '/tc                   open the pane',
].join('\n')
