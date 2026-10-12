import type { On } from 'claude-code'
import { describe, type Engine, expect, mock, test } from 'claude-code/testing'

const NOW = Date.parse('2026-10-12T09:00:00Z')

// Shapes as `tc --json` prints them (handoff of the thread "CLI and releases").
const STATUS = {
  connected: true,
  service: 'https://app.tokencontroller.com',
  organization: 'Kestrel Digital',
  person: 'mara@kestrel.example',
  agents: [{ agent_harness: 'claude-code', sessions: 2 }],
  rules: 3,
  today: { list_value: '51.62', currency: 'USD', share_on_tickets_percent: 81 },
  token_sheet: { period_start: '2026-10-05', period_end: '2026-10-11', late: false },
}

function entry(number: number, session: string, cost: string, tags: Record<string, string>) {
  return {
    number,
    agent_harness: 'claude-code',
    session_id: session,
    model: 'claude-opus-5',
    started_at: '2026-10-12T08:12:00Z',
    ended_at: '2026-10-12T08:50:00Z',
    tokens: { input: 4120, output: 610, cache_write: 0, cache_read: 18200 },
    list_value: cost,
    currency: 'USD',
    summary: number === 1 ? 'Fixed rounding in the checkout total' : '',
    status: 'draft',
    rejected_reason: null,
    placed_by: number === 1 ? 'rule: ticket key in branch' : 'rule: repository',
    candidates: [],
    splits: [{ share_percent: 100, list_value: cost, tags }],
  }
}

const REVIEW = {
  token_entries: [
    entry(1, 'session-1', '41.87', { ticket: 'KD-214', team: 'web' }),
    entry(2, 'session-0', '9.75', { project: 'storefront', team: 'web' }),
  ],
}

const REPORT = {
  by: 'ticket',
  currency: 'USD',
  rows: [
    { value: 'KD-214', list_value: '120.50', token_entries: 3 },
    { value: null, list_value: '80.25', token_entries: 2 },
  ],
  total: '200.75',
}

const SUBMITTED = {
  submitted: 2,
  list_value: '51.62',
  currency: 'USD',
  share_on_tickets_percent: 81,
}

type Run = { argv: readonly string[]; stdin: string | undefined }

/** The machine beneath the mod: a tc that answers from memory, and the session. */
function world(on: On, options: { isTcInstalled?: boolean } = {}) {
  const machine = { isTcInstalled: options.isTcInstalled ?? true }
  const clock = mock.clock(on, { now: NOW })
  mock.store(on)
  mock.env(on, { HOME: '/home/mara' })
  const runs: Run[] = []
  on('process.run', (_, e) => {
    const [program, ...args] = e.argv
    if (!machine.isTcInstalled) return { deny: `spawn ${program} ENOENT` }
    runs.push({ argv: e.argv, stdin: e.init?.stdin })
    const done = (stdout: string, exitCode = 0) => ({
      value: {
        exitCode,
        stdout,
        stderr: exitCode === 0 ? '' : stdout,
        isStdoutTruncated: false,
        isStderrTruncated: false,
      },
    })
    switch (args.join(' ')) {
      case 'status --json':
        return done(JSON.stringify(STATUS))
      case 'review --json':
        return done(JSON.stringify(REVIEW))
      case 'report --by ticket --json':
      case 'report --by day --json --filter ticket=KD-214':
        return done(JSON.stringify(REPORT))
      case 'record --json':
        return done(JSON.stringify({ token_entry: REVIEW.token_entries[0] }))
      case 'tag 2 ticket=KD-231 --json':
        return done(JSON.stringify({ token_entry: REVIEW.token_entries[1] }))
      case 'submit --json':
        return done(JSON.stringify(SUBMITTED))
      default:
        return done(`tc: unknown command ${args.join(' ')}`, 2)
    }
  })
  on('session.id', () => ({ value: 'session-1' }))
  on('session.messages', () => ({
    value: [
      { role: 'user', text: 'Fix the rounding in the checkout total for KD-214', toolUses: [] },
      { role: 'assistant', text: 'Fixed it and added tests.', toolUses: [] },
    ],
  }))
  on('model.complete', () => ({
    value: {
      isAnswered: true,
      text: 'Fixed rounding in the checkout total and added tests.',
      usage: {
        input_tokens: 300,
        output_tokens: 12,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
      },
    },
  }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('tool.register', (_, e) => ({ value: { tool: `mcp__token-controller__${e.name}` } }))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('ui.render', () => null as never)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  return { clock, runs, machine }
}

/** What the mod handed to `tc record`, one JSON object per run. */
function records(runs: Run[]): Array<Record<string, unknown>> {
  return runs
    .filter(run => run.argv[1] === 'record')
    .map(run => JSON.parse(run.stdin ?? '') as Record<string, unknown>)
}

function ran(runs: Run[], command: string): boolean {
  return runs.some(run => run.argv.slice(1).join(' ') === command)
}

async function start($: Engine) {
  await $.session.start({ cwd: '/work/storefront', surface: 'terminal', isInteractive: true })
}

const TURN = {
  answer: 'Fixed it.',
  durationMs: 4000,
  isAborted: false,
  turnId: 'turn-1',
  reason: 'answer' as const,
  usage: {
    model: 'claude-opus-5',
    input_tokens: 1200,
    output_tokens: 340,
    cache_read_input_tokens: 18_000,
    cache_creation_input_tokens: 2_000,
  },
}

describe('recording', () => {
  test('after each turn tc reads the session and the band gets its figures', async ($, on) => {
    const { clock, runs } = world(on)
    await start($)
    runs.length = 0
    await $.turn.complete({ ...TURN, reason: 'aborted' as const, isAborted: true })
    await clock.settle()

    // tc reads Claude Code's session files on every run; no token counts pass through the mod.
    expect(runs.map(run => run.argv.slice(1).join(' '))).toEqual(['status --json', 'review --json'])
  })

  test('the summary is written by a small model call and handed to tc', async ($, on) => {
    const { clock, runs } = world(on)
    await start($)
    await $.turn.complete(TURN)
    await clock.settle()

    expect(records(runs)).toEqual([
      {
        agent_harness: 'claude-code',
        session_id: 'session-1',
        summary: 'Fixed rounding in the checkout total and added tests',
      },
    ])
  })

  test('a headless run records its summary before the turn ends', async ($, on) => {
    const { runs } = world(on)
    await $.session.start({ cwd: '/work/storefront', surface: 'terminal', isInteractive: false })
    await $.turn.complete(TURN)

    expect(records(runs).map(r => r.summary)).toEqual([
      'Fixed rounding in the checkout total and added tests',
    ])
  })

  test('without tc the record waits on the machine and the band says how to install it', async ($, on) => {
    const { clock, runs, machine } = world(on, { isTcInstalled: false })
    await start($)
    await $.turn.complete(TURN)
    await clock.settle()

    const band = await $.ui.mount({
      plugin: 'token-controller',
      surface: 'terminal',
      component: 'AbovePrompt',
      props: { hasSurvey: false, isWorking: false, maxRows: 4, bodyColumns: 100 } as never,
    })
    expect(await band.find({ type: 'Text', text: /brew install tc/ })).toBeDefined()

    machine.isTcInstalled = true
    await $.turn.complete({
      ...TURN,
      turnId: 'turn-2',
      reason: 'aborted' as const,
      isAborted: true,
    })
    await clock.settle()
    await $.session.start({ cwd: '/work/storefront', surface: 'terminal', isInteractive: true })

    expect(records(runs).map(r => r.summary)).toEqual([
      'Fixed rounding in the checkout total and added tests',
    ])
  })
})

describe('commands', () => {
  const presentation = { isFullscreen: false, columns: 100 }
  const origin = { kind: 'composer' } as never

  test('/tc ticket sets the session ticket through tc', async ($, on) => {
    const { runs } = world(on)
    await start($)
    const result = await $.command.run({
      command: 'tc',
      args: 'ticket KD-231',
      origin,
      presentation,
    } as never)

    expect(result.text).toBe('This session: ticket KD-231')
    expect(records(runs)).toEqual([
      { agent_harness: 'claude-code', session_id: 'session-1', ticket: 'KD-231' },
    ])
  })

  test('/tc tag adds to what was set before, so each record is the whole picture', async ($, on) => {
    const { runs } = world(on)
    await start($)
    await $.command.run({ command: 'tc', args: 'ticket KD-231', origin, presentation } as never)
    await $.command.run({ command: 'tc', args: 'tag purpose=spike', origin, presentation } as never)

    expect(records(runs).at(-1)).toEqual({
      agent_harness: 'claude-code',
      session_id: 'session-1',
      ticket: 'KD-231',
      tags: { purpose: 'spike' },
    })
  })

  test('/tc report draws the report as a table with bars', async ($, on) => {
    world(on)
    await start($)
    const result = await $.command.run({
      command: 'tc',
      args: 'report',
      origin,
      presentation,
    } as never)

    expect(result.text).toMatch(/KD-214\s+\$120\.50\s+60%\s+█+/)
    expect(result.text).toMatch(/\(no ticket\)\s+\$80\.25\s+40%/)
    expect(result.text).toMatch(/total\s+\$200\.75/)
  })

  test('/tc submit submits the token sheet', async ($, on) => {
    world(on)
    await start($)
    const result = await $.command.run({
      command: 'tc',
      args: 'submit',
      origin,
      presentation,
    } as never)

    expect(result.text).toBe('Submitted 2 token entries · $51.62 · 81% on tickets')
  })

  test('/tc with a word it does not know shows the help', async ($, on) => {
    world(on)
    await start($)
    const result = await $.command.run({
      command: 'tc',
      args: 'budget',
      origin,
      presentation,
    } as never)

    expect(result.text).toMatch(/\/tc has no "budget"/)
    expect(result.text).toMatch(/\/tc ticket KD-214/)
  })
})

test('the model can ask for a report', async ($, on) => {
  world(on)
  await start($)
  const answer = await $.tool.call({
    tool: 'mcp__token-controller__report',
    input: { by: 'day', filter: 'ticket=KD-214' },
    tool_use_id: 'toolu_1',
  } as never)

  expect(String((answer as { result?: unknown }).result)).toMatch(
    /\| KD-214 \| \$120\.50 \| 60% \|/,
  )
})

describe('drawing', () => {
  for (const surface of ['terminal', 'desktop'] as const) {
    test(`the band shows the headline on the ${surface}`, async ($, on) => {
      world(on)
      await start($)
      const band = await $.ui.mount({
        plugin: 'token-controller',
        surface,
        component: 'AbovePrompt',
        props: { hasSurvey: false, isWorking: false, maxRows: 4, bodyColumns: 100 } as never,
      })

      expect(
        await band.find({ type: 'Text', text: '$51.62 today · 81% on tickets · KD-214' }),
      ).toBeDefined()
    })

    test(`the pane lists drafts, fixes a ticket and submits on the ${surface}`, async ($, on) => {
      const { runs } = world(on)
      await start($)
      const pane = await $.ui.mount({
        plugin: 'token-controller',
        surface,
        component: 'Pane',
        requestId: 'token-controller',
        props: { title: 'Token Controller', isFocused: true, bodyColumns: 80 } as never,
      })

      expect(await pane.find({ type: 'Button', key: 'pick-1' })).toBeDefined()
      expect(await pane.find({ type: 'Text', text: /Fixed rounding/ })).toBeDefined()

      await pane.press({ key: 'pick-2' })
      await pane.input({ key: 'tag-2', text: 'KD-231' })
      expect(ran(runs, 'tag 2 ticket=KD-231 --json')).toBe(true)

      await pane.press({ key: 'submit' })
      expect(ran(runs, 'submit --json')).toBe(true)
    })
  }
})
