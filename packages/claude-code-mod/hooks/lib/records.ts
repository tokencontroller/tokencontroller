/**
 * What the mod hands to `tc record`: one JSON object with what only Claude
 * Code knows about a session: its summary, the facts the mod counts, and the
 * tags the person set by hand. tc reads the session's messages with their
 * token counts, branches and prompt keys from Claude Code's own session files
 * and splits the session by the rules (ADR 0010). Each record is the whole
 * picture so far, so sending it twice changes nothing. Never prompt text.
 */
export interface SessionRecord {
  agent_harness: 'claude-code'
  session_id: string
  summary?: string
  /** Session facts the mod counts: turns, their duration, subagent turns. */
  facts?: { turns: number; duration_seconds: number; subagents: number }
  feature?: string
  /** Every tag but the feature the person set by hand this session. */
  tags?: Record<string, string>
}

export interface Facts {
  turns: number
  durationSeconds: number
  subagents: number
}

export const NO_FACTS: Facts = { turns: 0, durationSeconds: 0, subagents: 0 }

/** The facts after one more turn: a subagent's turn counts as a subagent, a main one as a turn. */
export function countTurn(facts: Facts, durationMs: number, isSubagent: boolean): Facts {
  return {
    turns: facts.turns + (isSubagent ? 0 : 1),
    durationSeconds: facts.durationSeconds + Math.max(0, durationMs) / 1000,
    subagents: facts.subagents + (isSubagent ? 1 : 0),
  }
}

export function sessionRecord(
  sessionId: string,
  summary: string | null,
  facts: Facts,
  handTags: Readonly<Record<string, string>>,
): SessionRecord {
  const { feature, ...tags } = handTags
  const hasFacts = facts.turns > 0 || facts.subagents > 0
  return {
    agent_harness: 'claude-code',
    session_id: sessionId,
    ...(summary === null ? {} : { summary }),
    ...(hasFacts
      ? {
          facts: {
            turns: facts.turns,
            duration_seconds: Math.round(facts.durationSeconds),
            subagents: facts.subagents,
          },
        }
      : {}),
    ...(feature === undefined ? {} : { feature }),
    ...(Object.keys(tags).length === 0 ? {} : { tags }),
  }
}

/**
 * Reads `/tc` arguments: `feature KD-214`, `tag purpose=spike`, `report team`,
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
    case 'feature':
      if (rest.length !== 1 || rest[0] === undefined) {
        return { kind: 'help', problem: 'Name one feature, such as /tc feature KD-214.' }
      }
      return { kind: 'tags', tags: { feature: rest[0] } }
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
      return { kind: 'report', by: rest[0] ?? 'feature' }
    case 'submit':
      return { kind: 'submit' }
    default:
      return { kind: 'help', problem: `/tc has no "${verb}".` }
  }
}

export const HELP = [
  '/tc feature KD-214    put this session on a feature',
  '/tc tag purpose=spike set any other tag',
  '/tc report [by]       a report table, by feature, project, any tag or day',
  '/tc submit            submit everything since your last submit',
  '/tc                   open the pane',
].join('\n')
