import type { EngineInterface, Register } from 'claude-code'
import { atom, read, update } from 'claude-code'

import type { Tab } from '../types'
import {
  assignedLine,
  bar,
  headline,
  money,
  percent,
  reportTable,
  reportText,
  rowLabel,
  sessionFeature,
  splitLine,
  statusNote,
  submittedText,
  when,
} from './lib/format.ts'
import { countTurn, HELP, NO_FACTS, parseCommand, sessionRecord } from './lib/records.ts'
import { cleanSummary, SUMMARY_MODEL, SUMMARY_SYSTEM, summaryPrompt } from './lib/summary.ts'
import {
  parseJson,
  readReport,
  readReview,
  readRules,
  readStatus,
  readSubmitted,
  reason,
} from './lib/tc.ts'

type $ = EngineInterface

const PANE = 'token-controller'
const TOOL = 'mcp__token-controller__report'
const INSTALL =
  'Install tc to keep your messages: brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller && brew install tc'

const status = atom({ plugin: 'token-controller', key: 'status' } as const, null)
const review = atom({ plugin: 'token-controller', key: 'review' } as const, [])
const report = atom({ plugin: 'token-controller', key: 'report' } as const, null)
const reportBy = atom({ plugin: 'token-controller', key: 'reportBy' } as const, 'feature')
const rules = atom({ plugin: 'token-controller', key: 'rules' } as const, [])
const tab = atom({ plugin: 'token-controller', key: 'tab' } as const, 'review')
const editing = atom({ plugin: 'token-controller', key: 'editing' } as const, null)
const notice = atom({ plugin: 'token-controller', key: 'notice' } as const, null)
const problem = atom({ plugin: 'token-controller', key: 'problem' } as const, null)
const feature = atom({ plugin: 'token-controller', key: 'feature' } as const, null)
const handTags = atom({ plugin: 'token-controller', key: 'handTags' } as const, {})
const summary = atom({ plugin: 'token-controller', key: 'summary' } as const, null)
const facts = atom({ plugin: 'token-controller', key: 'facts' } as const, NO_FACTS)

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

/** Runs one thing after another, so store writes never overlap. */
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

/**
 * Keeps this session's record in the machine's store until tc has taken it.
 * The record is the whole picture so far, so only the newest one waits.
 */
function remember($: $): Promise<void> {
  return serial(async () => {
    const [sessionId, line, counted, tags] = await Promise.all([
      $.session.id(),
      read($, summary),
      read($, facts),
      read($, handTags),
    ])
    await $.store.set(`record/${sessionId}`, sessionRecord(sessionId, line, counted, tags))
  })
}

/** Hands every waiting record, this session's and earlier ones', to tc. */
function flush($: $): Promise<void> {
  return serial(async () => {
    const waiting = (await $.store.keys()).filter(key => key.startsWith('record/'))
    for (const key of waiting) {
      const record = await $.store.get(key)
      if (record !== undefined) {
        const run = await tc($, ['record', '--json'], `${JSON.stringify(record)}\n`)
        if (!run.ok) {
          await setProblem($, run)
          return
        }
      }
      await $.store.delete(key)
    }
    await setProblem($, null)
  })
}

/** Reads today's figures and the check list from tc for the band and the pane. */
async function refresh($: $): Promise<void> {
  const run = await tc($, ['status', '--json'])
  if (!run.ok) {
    await setProblem($, run)
    return
  }
  await update($, status, () => readStatus(parseJson(run.stdout)))
  const listed = await tc($, ['review', '--json'])
  if (listed.ok) {
    const rows = readReview(parseJson(listed.stdout))
    await update($, review, () => rows)
    const split = sessionFeature(rows, await $.session.id())
    await update($, feature, () => split)
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

/**
 * After a turn: the mod counts the turn into the session facts and writes the
 * summary; tc reads the session's messages from Claude Code's own session
 * files whenever it runs, so asking it for the band's figures is what brings
 * the turn into the session record.
 */
async function afterTurn($: $, isMain: boolean): Promise<void> {
  if (isMain) await summarize($)
  else await remember($)
  await flush($)
  if (isInteractive) await refresh($)
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
    await remember($)
  } finally {
    isSummarizing = false
  }
}

async function setTags($: $, tags: Record<string, string>): Promise<string> {
  await update($, handTags, current => ({ ...current, ...tags }))
  await remember($)
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
  const run = await tc($, ['submit', '--json'])
  await refresh($)
  if (run.missing) return INSTALL
  return run.ok
    ? submittedText(readSubmitted(parseJson(run.stdout)))
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
        'Token Controller: put this session on a feature, see reports, submit your sessions',
      argumentHint: '[feature KEY | tag NAME=VALUE | report [BY] | submit]',
    })
    await $.tool.register({
      name: 'report',
      description:
        'Token Controller report: what agent work cost, grouped by feature, project, team, any other tag or attribute name, or day. ' +
        'Use it when the person asks what something cost, such as "what did KD-214 cost this week?" ' +
        '(by day, filter feature=KD-214, then add up the days of this week), and answer with the table it returns.',
      inputSchema: {
        type: 'object',
        properties: {
          by: { type: 'string', description: 'feature, project, team, any tag name, or day' },
          filter: {
            type: 'string',
            description: 'Limit to one tag value, as name=value, such as feature=KD-214',
          },
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

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    const isMain = e.agentId === undefined && e.reason === 'answer'
    await update($, facts, counted => countTurn(counted, e.durationMs, e.agentId !== undefined))
    if (isInteractive) {
      // Outside the turn's dispatch, so tc and the summary never hold up the prompt.
      $.clock.after(0, () => void afterTurn($, isMain))
    } else if (isMain) {
      // A headless run (claude -p, CI) ends with its turn: record before it does.
      await afterTurn($, isMain)
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
    const input = e.input as { by?: unknown; filter?: unknown }
    const by = typeof input.by === 'string' && input.by !== '' ? input.by : 'feature'
    const args = ['report', '--by', by, '--json']
    if (typeof input.filter === 'string' && input.filter !== '') args.push('--filter', input.filter)
    const run = await tc($, args)
    if (run.missing) return { result: INSTALL }
    if (!run.ok) return { result: reason(run.stderr, run.stdout, 'tc report failed') }
    return { result: reportTable(readReport(parseJson(run.stdout), by)) }
  }).catch(() => ({ deny: 'Token Controller could not make the report.' }))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.props.hasSurvey) return next(e)
    const [current, trouble, featureNow] = await Promise.all([
      read($, status),
      read($, problem),
      read($, feature),
    ])
    if (current === null && trouble === null) return next(e)
    const { Box, Text, Button } = $.ui.resolve(e)
    const note = current === null ? null : statusNote(current)
    // A note gets its own row when the band may take two, so the headline never wraps.
    const isNoteBelow = note !== null && e.props.maxRows >= 2
    const top = (
      <Box flexDirection="row" gap={1}>
        {current !== null && <Text wrap="truncate-end">{headline(current, featureNow)}</Text>}
        {note !== null && !isNoteBelow && (
          <Text color="warning" wrap="truncate-end">
            · {note}
          </Text>
        )}
        {trouble !== null && (
          <Text color="warning" wrap="truncate-end">
            {trouble}
          </Text>
        )}
        <Button key="open" plain dimColor label="details" onPress={() => void openPane($)} />
      </Box>
    )
    if (!isNoteBelow) return top
    return (
      <Box flexDirection="column">
        {top}
        <Text color="warning" wrap="truncate-end">
          {note}
        </Text>
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const elements = $.ui.resolve(e)
    const { Box, Text, Button } = elements
    // The mobile app draws no text field yet: there the pane shows and submits, and fixes wait for a computer.
    const Input = 'Input' in elements ? elements.Input : undefined
    const [current, trouble, shown, rows, chosen, said, featureNow, now] = await Promise.all([
      read($, status),
      read($, problem),
      read($, tab),
      read($, review),
      read($, editing),
      read($, notice),
      read($, feature),
      $.clock.now(),
    ])
    const width = Math.max(30, e.props.bodyColumns)
    const note = current === null ? null : statusNote(current)
    const toCheck = rows.filter(row => !row.isRejected)
    const rejected = rows.length - toCheck.length
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
        {current !== null && <Text bold>{headline(current, featureNow)}</Text>}
        {current !== null && (
          <Text dimColor wrap="truncate-end">
            {[
              current.organization ?? 'Local only: tc connect to report to your organization',
              `${toCheck.length} ${toCheck.length === 1 ? 'session' : 'sessions'} to check`,
              ...(rejected > 0 ? [`${rejected} rejected`] : []),
            ].join(' · ')}
          </Text>
        )}
        {note !== null && <Text color="warning">{note}</Text>}
        {trouble !== null && <Text color="warning">{trouble}</Text>}
        <Box flexDirection="row" gap={1} marginTop={1}>
          <Button
            key="tab-review"
            label="Check"
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
      const choices = ['feature', 'project', 'team', 'day']
      const options = choices.includes(by) ? choices : [...choices, by]
      const labels = shownReport === null ? [] : shownReport.rows.map(r => rowLabel(r, by))
      const label = Math.min(20, Math.max(5, ...labels.map(l => l.length)))
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
              {shownReport.rows.map((row, i) => (
                <Box key={`row-${row.value ?? ''}`} flexDirection="row" gap={1}>
                  <Text wrap="truncate-end">{(labels[i] ?? '').slice(0, label).padEnd(label)}</Text>
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
                <Text wrap="truncate-end">{rule.text}</Text>
                <Button
                  key={`remove-${rule.number}`}
                  plain
                  dimColor
                  label="remove"
                  onPress={async () => {
                    const run = await tc($, ['rules', 'remove', String(rule.number), '--json'])
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
              placeholder="branch starts-with review/ purpose=code-review"
              submitLabel="Add"
              onSubmit={async (text: string) => {
                if (text.trim() === '') return
                const run = await tc($, ['rules', 'add', text.trim(), '--json'])
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

    const total = toCheck.reduce((sum, row) => sum + (row.cost ?? 0), 0)
    const currency = current?.currency ?? 'USD'
    const costText = (row: (typeof rows)[number]) =>
      row.cost === null ? 'no price' : money(row.cost, currency, true)
    const whenWidth = Math.max(0, ...rows.map(row => when(row.startedAt, now).length))
    const costWidth = Math.max(0, ...rows.map(row => costText(row).length))
    return (
      <Box flexDirection="column" gap={1}>
        {header}
        {rows.length === 0 && (
          <Text dimColor>No sessions to check. Each Claude Code session shows here.</Text>
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
                {costText(row).padStart(costWidth)} {splitLine(row)}
              </Button>
              {row.summary !== '' && (
                <Text wrap="truncate-end">
                  {'   '}
                  {row.summary}
                </Text>
              )}
              <Text
                dimColor={!row.isRejected}
                color={row.isRejected ? 'warning' : undefined}
                wrap="truncate-end"
              >
                {'   '}
                {assignedLine(row)}
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
                      .map((pair: string) => (pair.includes('=') ? pair : `feature=${pair}`))
                    if (pairs.length === 0) return
                    const run = await tc($, ['tag', String(row.number), ...pairs, '--json'])
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
        {toCheck.length > 0 && (
          <Box flexDirection="row" gap={1}>
            <Button
              key="submit"
              variant="primary"
              hotkey="s"
              label={`Submit ${toCheck.length} ${toCheck.length === 1 ? 'session' : 'sessions'} · ${money(total, currency, true)}`}
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
