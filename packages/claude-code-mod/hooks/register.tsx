import type { EngineInterface, Register } from 'claude-code'
import { atom, read, update } from 'claude-code'

import type { Tab } from '../types'
import {
  bar,
  headline,
  money,
  percent,
  reportTable,
  reportText,
  sessionTicket,
  sheetNote,
  tagLine,
  when,
} from './lib/format.ts'
import {
  addToOutbox,
  HELP,
  parseCommand,
  repositoryName,
  type TcRecord,
  ticketKeyCandidates,
  turnRecord,
} from './lib/records.ts'
import { cleanSummary, SUMMARY_MODEL, SUMMARY_SYSTEM, summaryPrompt } from './lib/summary.ts'
import { parseJson, readReport, readReview, readRules, readSheet, reason } from './lib/tc.ts'

type $ = EngineInterface

const PANE = 'token-controller'
const TOOL = 'mcp__token-controller__report'
const INSTALL =
  'Install tc to keep your token entries: brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller && brew install tc'

const sheet = atom({ plugin: 'token-controller', key: 'sheet' } as const, null)
const review = atom({ plugin: 'token-controller', key: 'review' } as const, [])
const report = atom({ plugin: 'token-controller', key: 'report' } as const, null)
const reportBy = atom({ plugin: 'token-controller', key: 'reportBy' } as const, 'ticket')
const rules = atom({ plugin: 'token-controller', key: 'rules' } as const, [])
const tab = atom({ plugin: 'token-controller', key: 'tab' } as const, 'review')
const editing = atom({ plugin: 'token-controller', key: 'editing' } as const, null)
const notice = atom({ plugin: 'token-controller', key: 'notice' } as const, null)
const problem = atom({ plugin: 'token-controller', key: 'problem' } as const, null)
const ticket = atom({ plugin: 'token-controller', key: 'ticket' } as const, null)
const keys = atom({ plugin: 'token-controller', key: 'keys' } as const, [])
const summary = atom({ plugin: 'token-controller', key: 'summary' } as const, null)
const cost = atom({ plugin: 'token-controller', key: 'cost' } as const, null)

/** Where tc may be when the session's PATH does not have it (the desktop app). */
const FALLBACK_PATHS = [
  '/opt/homebrew/bin/tc',
  '/usr/local/bin/tc',
  '/home/linuxbrew/.linuxbrew/bin/tc',
]

type TcRun = { ok: boolean; missing: boolean; stdout: string; stderr: string }

// The module's own values start over when it reloads; nothing here needs to last.
let tcPath: string | undefined
let queue: Promise<unknown> = Promise.resolve()
let isSummarizing = false
let isInteractive = true

/** Runs one thing after another, so outbox writes never overlap. */
function serial<T>(work: () => Promise<T>): Promise<T> {
  const run = queue.then(work, work)
  queue = run.catch(() => undefined)
  return run
}

async function tc($: $, args: string[], stdin?: string): Promise<TcRun> {
  const home = await $.env.get('HOME')
  const paths = tcPath
    ? [tcPath]
    : ['tc', ...FALLBACK_PATHS, ...(home ? [`${home}/.local/bin/tc`, `${home}/.cargo/bin/tc`] : [])]
  for (const path of paths) {
    try {
      const run = await $.process.run([path, ...args], {
        env: { NO_COLOR: '1' },
        timeoutMs: 20_000,
        ...(stdin === undefined ? {} : { stdin }),
      })
      tcPath = path
      return { ok: run.exitCode === 0, missing: false, stdout: run.stdout, stderr: run.stderr }
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error)
      if (!/ENOENT|not found|no such file|cannot (start|find)|spawn/i.test(text)) {
        return { ok: false, missing: false, stdout: '', stderr: text }
      }
    }
  }
  return { ok: false, missing: true, stdout: '', stderr: '' }
}

async function setProblem($: $, run: TcRun | null): Promise<void> {
  const text =
    run === null ? null : run.missing ? INSTALL : reason(run.stderr, run.stdout, 'tc failed')
  await update($, problem, () => text)
}

/** Keeps records in the machine's store until tc has taken them. */
function remember($: $, ...records: TcRecord[]): Promise<void> {
  return serial(async () => {
    const key = `outbox/${await $.session.id()}`
    const outbox = ((await $.store.get(key)) as string[] | undefined) ?? []
    await $.store.set(key, addToOutbox(outbox, ...records))
  })
}

/** Hands every waiting record, this session's and earlier ones', to tc. */
function flush($: $): Promise<void> {
  return serial(async () => {
    const outboxes = (await $.store.keys()).filter(key => key.startsWith('outbox/'))
    for (const key of outboxes) {
      const lines = ((await $.store.get(key)) as string[] | undefined) ?? []
      if (lines.length === 0) {
        await $.store.delete(key)
        continue
      }
      const run = await tc($, ['record'], `${lines.join('\n')}\n`)
      if (!run.ok) {
        await setProblem($, run)
        return
      }
      await $.store.delete(key)
    }
    await setProblem($, null)
  })
}

/** Reads the token sheet and the review list from tc for the band and the pane. */
async function refresh($: $): Promise<void> {
  const status = await tc($, ['status', '--json'])
  if (!status.ok) {
    await setProblem($, status)
    return
  }
  await update($, sheet, () => readSheet(parseJson(status.stdout)))
  const listed = await tc($, ['review', '--json'])
  if (listed.ok) {
    const rows = readReview(parseJson(listed.stdout))
    await update($, review, () => rows)
    const placed = sessionTicket(rows, await $.session.id())
    if (placed !== null) await update($, ticket, current => current ?? placed)
  }
}

async function refreshReport($: $): Promise<void> {
  const by = await read($, reportBy)
  const run = await tc($, ['report', '--by', by, '--json'])
  if (run.ok) await update($, report, () => readReport(parseJson(run.stdout), by))
  else await update($, notice, () => reason(run.stderr, run.stdout, 'tc report failed'))
}

async function refreshRules($: $): Promise<void> {
  const run = await tc($, ['rules', '--json'])
  if (run.ok) await update($, rules, () => readRules(parseJson(run.stdout)))
}

async function git($: $, cwd: string, args: string[]): Promise<string | null> {
  try {
    const run = await $.process.run(['git', ...args], { cwd, timeoutMs: 5_000 })
    const out = run.stdout.trim()
    return run.exitCode === 0 && out !== '' ? out : null
  } catch {
    return null
  }
}

async function recordTurn(
  $: $,
  e: { turnId: string; agentId?: string; usage?: Parameters<typeof turnRecord>[0]['usage'] },
): Promise<void> {
  if (e.usage === undefined) return
  const usage = e.usage
  const [sessionId, cwd, at, used, costBefore, seen] = await Promise.all([
    $.session.id(),
    $.session.cwd(),
    $.clock.now(),
    $.session.usage(),
    read($, cost),
    read($, keys),
  ])
  const [branch, remote] = await Promise.all([
    git($, cwd, ['rev-parse', '--abbrev-ref', 'HEAD']),
    git($, cwd, ['config', '--get', 'remote.origin.url']),
  ])
  const costAfter = used.cost?.usd
  await remember(
    $,
    turnRecord({
      sessionId,
      turnId: e.turnId,
      agentId: e.agentId,
      at,
      usage,
      costBefore: costBefore ?? undefined,
      costAfter,
      cwd,
      branch: branch === 'HEAD' ? null : branch,
      repository: repositoryName(remote),
      ticketKeys: seen,
    }),
  )
  if (costAfter !== undefined) await update($, cost, () => costAfter)
  await flush($)
  await refresh($)
}

/** Writes the one-line summary with a small model call of the mod's own. */
async function summarize($: $): Promise<void> {
  if (isSummarizing) return
  isSummarizing = true
  try {
    const messages = (await $.session.messages()).map(m => ({ role: m.role, text: m.text }))
    const previous = await read($, summary)
    const answer = await $.model.complete({
      model: SUMMARY_MODEL,
      system: SUMMARY_SYSTEM,
      prompt: summaryPrompt(previous, messages.slice(-12)),
      maxTokens: 120,
    })
    const line = answer.isAnswered ? cleanSummary(answer.text) : null
    if (line === null || line === previous) return
    await update($, summary, () => line)
    await remember($, { kind: 'summary', session_id: await $.session.id(), summary: line })
    await flush($)
  } finally {
    isSummarizing = false
  }
}

async function setTags($: $, tags: Record<string, string>): Promise<string> {
  await remember($, { kind: 'tags', session_id: await $.session.id(), tags })
  if (tags.ticket !== undefined) await update($, ticket, () => tags.ticket ?? null)
  await flush($)
  await refresh($)
  const problemNow = await read($, problem)
  const set = Object.entries(tags)
    .map(([name, value]) => `${name} ${value}`)
    .join(' · ')
  return problemNow === null ? `This session: ${set}` : `This session: ${set}. ${problemNow}`
}

async function submit($: $): Promise<string> {
  await flush($)
  const run = await tc($, ['submit'])
  await refresh($)
  if (run.missing) return INSTALL
  return run.ok
    ? run.stdout.trim() || 'Submitted.'
    : reason(run.stderr, run.stdout, 'tc submit failed')
}

function openPane($: $) {
  return $.ui.open({ id: PANE, title: 'Token Controller' })
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    isInteractive = e.isInteractive
    await $.command.register({
      name: 'tc',
      description:
        'Token Controller: set this session’s ticket, see reports, submit your token sheet',
      argumentHint: '[ticket KEY | tag NAME=VALUE | report [BY] | submit]',
    })
    await $.tool.register({
      name: 'report',
      description:
        'Token Controller report: what agent work cost, grouped by ticket, project, team, any other tag name, or day. ' +
        'Use it when the person asks what something cost, such as "what did KD-214 cost this week?", ' +
        'and answer with the table it returns.',
      inputSchema: {
        type: 'object',
        properties: {
          by: { type: 'string', description: 'ticket, project, team, any tag name, or day' },
          filter: {
            type: 'string',
            description: 'Limit to one tag value, as name=value, such as ticket=KD-214',
          },
          since: { type: 'string', description: 'First day to count, as YYYY-MM-DD' },
        },
        required: ['by'],
      },
      isDeferred: false,
    })
    // Records left by sessions that ended before tc took them go first.
    await flush($)
    await refresh($)
    $.clock.every(5 * 60_000, () => {
      void refresh($)
    })
    return started
  })

  on('prompt.submit', async ($, e, next) => {
    const found = ticketKeyCandidates(e.text)
    if (found.length > 0) await update($, keys, seen => [...new Set([...seen, ...found])])
    return next(e)
  }).catch((_, e, next) => next(e)) // Never stand between the person and their prompt.

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    const isMain = e.agentId === undefined && e.reason === 'answer'
    const work = async () => {
      await recordTurn($, e)
      if (isMain) await summarize($)
    }
    if (isInteractive) {
      // Outside the turn's dispatch, so tc and the summary never hold up the prompt.
      $.clock.after(0, () => void work())
    } else {
      // A headless run (claude -p, CI) ends with its turn: record before it does.
      await work()
    }
    return result
  })

  on('session.end', async ($, e, next) => {
    // All session.end hooks share 1.5 seconds. What tc does not take now
    // stays in the store and goes at the next session's start.
    await Promise.race([flush($), $.clock.sleep(1_000)])
    return next(e)
  })

  on('command.run', { command: 'tc' }, async ($, e) => {
    const command = parseCommand(e.args)
    switch (command.kind) {
      case 'open':
        await openPane($)
        return { text: 'Opened the Token Controller pane.' }
      case 'tags':
        return { text: await setTags($, command.tags) }
      case 'report': {
        const run = await tc($, ['report', '--by', command.by, '--json'])
        if (run.missing) return { text: INSTALL }
        if (!run.ok) return { text: reason(run.stderr, run.stdout, 'tc report failed') }
        return {
          text: reportText(
            readReport(parseJson(run.stdout), command.by),
            e.presentation.columns - 4,
          ),
        }
      }
      case 'submit':
        return { text: await submit($) }
      case 'help':
        return { text: [command.problem, HELP].filter(Boolean).join('\n\n') }
    }
  })

  on('tool.call', { tool: TOOL }, async ($, e) => {
    const input = e.input as { by?: unknown; filter?: unknown; since?: unknown }
    const by = typeof input.by === 'string' && input.by !== '' ? input.by : 'ticket'
    const args = ['report', '--by', by, '--json']
    if (typeof input.filter === 'string' && input.filter !== '') args.push('--filter', input.filter)
    if (typeof input.since === 'string' && input.since !== '') args.push('--since', input.since)
    const run = await tc($, args)
    if (run.missing) return { result: INSTALL }
    if (!run.ok) return { result: reason(run.stderr, run.stdout, 'tc report failed') }
    return { result: reportTable(readReport(parseJson(run.stdout), by)) }
  }).catch(() => ({ deny: 'Token Controller could not make the report.' }))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const [current, trouble, sessionTicketNow, now] = await Promise.all([
      read($, sheet),
      read($, problem),
      read($, ticket),
      $.clock.now(),
    ])
    if (current === null && trouble === null) return next(e)
    const { Box, Text, Button } = $.ui.resolve(e)
    const note = current === null ? null : sheetNote(current, now)
    return (
      <Box flexDirection="row" gap={1}>
        {current !== null && <Text>{headline(current, sessionTicketNow)}</Text>}
        {note !== null && <Text color={current?.isLate ? 'warning' : 'subtle'}>· {note}</Text>}
        {trouble !== null && (
          <Text color="warning" wrap="truncate-end">
            {trouble}
          </Text>
        )}
        <Button key="open" plain dimColor label="details" onPress={() => void openPane($)} />
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const elements = $.ui.resolve(e)
    const { Box, Text, Button } = elements
    // The mobile app draws no text field yet: there the pane shows and submits, and fixes wait for a computer.
    const Input = 'Input' in elements ? elements.Input : undefined
    const [current, trouble, shown, rows, chosen, said, sessionTicketNow, now] = await Promise.all([
      read($, sheet),
      read($, problem),
      read($, tab),
      read($, review),
      read($, editing),
      read($, notice),
      read($, ticket),
      $.clock.now(),
    ])
    const width = Math.max(30, e.props.bodyColumns)
    const note = current === null ? null : sheetNote(current, now)
    const show = (next: Tab) => async () => {
      await update($, tab, () => next)
      await update($, notice, () => null)
      if (next === 'report') await refreshReport($)
      if (next === 'rules') await refreshRules($)
      if (next === 'review') await refresh($)
    }
    const say = (text: string) => update($, notice, () => text)

    const header = (
      <Box flexDirection="column">
        {current !== null && <Text bold>{headline(current, sessionTicketNow)}</Text>}
        {current !== null && (
          <Text dimColor wrap="truncate-end">
            {[
              current.organization ?? 'Local only: tc connect to report to your organization',
              `${current.tokenEntries} draft token ${current.tokenEntries === 1 ? 'entry' : 'entries'}`,
            ].join(' · ')}
          </Text>
        )}
        {note !== null && <Text color={current?.isLate ? 'warning' : 'subtle'}>{note}</Text>}
        {trouble !== null && <Text color="warning">{trouble}</Text>}
        <Box flexDirection="row" gap={1} marginTop={1}>
          <Button
            key="tab-review"
            label="Review"
            hotkey="r"
            variant={shown === 'review' ? 'primary' : 'secondary'}
            onPress={show('review')}
          />
          <Button
            key="tab-report"
            label="Report"
            hotkey="p"
            variant={shown === 'report' ? 'primary' : 'secondary'}
            onPress={show('report')}
          />
          <Button
            key="tab-rules"
            label="Rules"
            hotkey="u"
            variant={shown === 'rules' ? 'primary' : 'secondary'}
            onPress={show('rules')}
          />
        </Box>
        {said !== null && <Text dimColor>{said}</Text>}
      </Box>
    )

    if (shown === 'report') {
      const [shownReport, by] = await Promise.all([read($, report), read($, reportBy)])
      const choices = ['ticket', 'project', 'team', 'day']
      const options = choices.includes(by) ? choices : [...choices, by]
      const label =
        shownReport === null
          ? 0
          : Math.min(20, Math.max(4, ...shownReport.rows.map(r => r.value.length)))
      const barWidth = Math.max(6, width - label - 22)
      const max = shownReport === null ? 0 : Math.max(0, ...shownReport.rows.map(r => r.cost))
      return (
        <Box flexDirection="column" gap={1}>
          {header}
          <Box flexDirection="row" gap={1}>
            {options.map(option => (
              <Button
                key={`by-${option}`}
                label={option}
                variant={option === by ? 'primary' : 'secondary'}
                onPress={async () => {
                  await update($, reportBy, () => option)
                  await refreshReport($)
                }}
              />
            ))}
          </Box>
          {shownReport === null && <Text dimColor>No report yet.</Text>}
          {shownReport !== null && shownReport.rows.length === 0 && (
            <Text dimColor>No cost by {shownReport.by} yet.</Text>
          )}
          {shownReport !== null && shownReport.rows.length > 0 && (
            <Box flexDirection="column">
              {shownReport.rows.map(row => (
                <Box key={`row-${row.value}`} flexDirection="row" gap={1}>
                  <Text wrap="truncate-end">{row.value.slice(0, label).padEnd(label)}</Text>
                  <Text>{money(row.cost, shownReport.currency, true).padStart(10)}</Text>
                  <Text dimColor>{(row.share === null ? '' : percent(row.share)).padStart(4)}</Text>
                  <Text color="claude">{bar(row.cost, max, barWidth)}</Text>
                </Box>
              ))}
              <Text bold>
                {'total'.padEnd(label)}{' '}
                {money(shownReport.total, shownReport.currency, true).padStart(10)}
              </Text>
            </Box>
          )}
        </Box>
      )
    }

    if (shown === 'rules') {
      const listed = await read($, rules)
      return (
        <Box flexDirection="column" gap={1}>
          {header}
          {listed.length === 0 && (
            <Text dimColor>No personal rules yet. Your organization’s rules still apply.</Text>
          )}
          <Box flexDirection="column">
            {listed.map(rule => (
              <Box key={`rule-${rule.number}`} flexDirection="row" gap={1}>
                <Text dimColor>{String(rule.number).padStart(2)}</Text>
                <Text wrap="truncate-end">{rule.rule}</Text>
                <Button
                  key={`remove-${rule.number}`}
                  plain
                  dimColor
                  label="remove"
                  onPress={async () => {
                    const run = await tc($, ['rules', 'remove', String(rule.number)])
                    await say(
                      run.ok
                        ? `Removed rule ${rule.number}.`
                        : reason(run.stderr, run.stdout, 'tc rules failed'),
                    )
                    await refreshRules($)
                  }}
                />
              </Box>
            ))}
          </Box>
          {Input !== undefined && (
            <Input
              key="add-rule"
              label="Add a rule"
              placeholder="branch ^review/ → purpose=code-review"
              submitLabel="Add"
              onSubmit={async (text: string) => {
                if (text.trim() === '') return
                const run = await tc($, ['rules', 'add', text.trim()])
                await say(
                  run.ok ? 'Added the rule.' : reason(run.stderr, run.stdout, 'tc rules failed'),
                )
                await refreshRules($)
              }}
            />
          )}
        </Box>
      )
    }

    const total = rows.reduce((sum, row) => sum + row.cost, 0)
    const currency = current?.currency ?? 'USD'
    const whenWidth = Math.max(0, ...rows.map(row => when(row.startedAt, now).length))
    const costWidth = Math.max(0, ...rows.map(row => money(row.cost, currency, true).length))
    return (
      <Box flexDirection="column" gap={1}>
        {header}
        {rows.length === 0 && (
          <Text dimColor>No draft token entries. Each Claude Code session becomes one.</Text>
        )}
        <Box flexDirection="column">
          {rows.map(row => (
            <Box key={`entry-${row.number}`} flexDirection="column">
              <Button
                key={`pick-${row.number}`}
                plain
                onPress={() =>
                  update($, editing, open => (open === row.number ? null : row.number))
                }
              >
                {String(row.number).padStart(2)} {when(row.startedAt, now).padEnd(whenWidth)}{' '}
                {money(row.cost, currency, true).padStart(costWidth)}{' '}
                {tagLine(row.tags) || 'no tags'}
              </Button>
              {row.summary !== '' && (
                <Text wrap="truncate-end">
                  {'   '}
                  {row.summary}
                </Text>
              )}
              <Text dimColor wrap="truncate-end">
                {'   '}
                {row.by}
              </Text>
              {Input !== undefined && chosen === row.number && (
                <Input
                  key={`tag-${row.number}`}
                  label={`Tags for #${row.number}`}
                  placeholder="KD-231, or name=value"
                  submitLabel="Set"
                  autoFocus
                  onSubmit={async (text: string) => {
                    const pairs = text
                      .split(/\s+/)
                      .filter(Boolean)
                      .map((pair: string) => (pair.includes('=') ? pair : `ticket=${pair}`))
                    if (pairs.length === 0) return
                    const run = await tc($, ['tag', String(row.number), ...pairs])
                    await say(
                      run.ok
                        ? `#${row.number}: ${pairs.join(' ')}`
                        : reason(run.stderr, run.stdout, 'tc tag failed'),
                    )
                    await update($, editing, () => null)
                    await refresh($)
                  }}
                />
              )}
            </Box>
          ))}
        </Box>
        {rows.length > 0 && (
          <Box flexDirection="row" gap={1}>
            <Button
              key="submit"
              variant="primary"
              hotkey="s"
              label={`Submit ${rows.length} ${rows.length === 1 ? 'entry' : 'entries'} · ${money(total, currency, true)}`}
              onPress={async () => {
                await say(await submit($))
              }}
            />
          </Box>
        )}
      </Box>
    )
  })
}
