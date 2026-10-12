/**
 * What the mod hands to `tc record`: one JSON object with what only Claude
 * Code knows about a session, its summary and the tags the person set by
 * hand. tc reads the session's usage, branches and prompts from Claude Code's
 * own session files, builds the draft token entry and runs the rules (ADR
 * 0010). Each record is the whole picture so far, so sending it twice changes
 * nothing.
 */
export interface SessionRecord {
  agent_harness: 'claude-code'
  session_id: string
  summary?: string
  ticket?: string
  /** Every tag but the ticket the person set by hand this session. */
  tags?: Record<string, string>
}

export function sessionRecord(
  sessionId: string,
  summary: string | null,
  handTags: Readonly<Record<string, string>>,
): SessionRecord {
  const { ticket, ...tags } = handTags
  return {
    agent_harness: 'claude-code',
    session_id: sessionId,
    ...(summary === null ? {} : { summary }),
    ...(ticket === undefined ? {} : { ticket }),
    ...(Object.keys(tags).length === 0 ? {} : { tags }),
  }
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
