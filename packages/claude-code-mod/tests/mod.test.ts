import type { On } from 'claude-code'
import { describe, type Engine, expect, mock, test } from 'claude-code/testing'

const NOW = Date.parse('2026-10-12T09:00:00Z')

const STATUS = {
  organization: 'Kestrel',
  person: 'Mara',
  currency: 'USD',
  token_sheet: {
    token_entries: 5,
    cost: 212.4,
    share_on_tickets: 0.91,
    due_at: '2026-10-16T17:00:00Z',
    is_late: false,
  },
}

const REVIEW = [
  {
    number: 1,
    session_id: 'session-1',
    started_at: '2026-10-12T08:12:00Z',
    cost: 41.87,
    tags: { ticket: 'KD-214', team: 'web' },
    by: 'rule: ticket key in branch',
    summary: 'Fixed rounding in the checkout total',
  },
  {
    number: 2,
    session_id: 'session-0',
    started_at: '2026-10-11T15:20:00Z',
    cost: 9.75,
    tags: { project: 'storefront', team: 'web' },
    by: 'rule: repository',
    summary: '',
  },
]

const REPORT = {
  by: 'ticket',
  currency: 'USD',
  rows: [
    { value: 'KD-214', cost: 120.5, share: 0.6 },
    { value: 'KD-231', cost: 80.25, share: 0.4 },
  ],
  total: 200.75,
}

type Run = { argv: readonly string[]; stdin: string | undefined }

/** The machine beneath the mod: a tc that answers from memory, git, and the session. */
function world(on: On, options: { isTcInstalled?: boolean } = {}) {
  const machine = { isTcInstalled: options.isTcInstalled ?? true }
  const clock = mock.clock(on, { now: NOW })
  mock.store(on)
  mock.env(on, { HOME: '/home/mara' })
  const runs: Run[] = []
  let cost = 1.25
  on('process.run', (_, e) => {
    const [program, ...args] = e.argv
    if (program !== 'git' && !machine.isTcInstalled) return { deny: `spawn ${program} ENOENT` }
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
    if (program === 'git') {
      if (args[0] === 'rev-parse') return done('kd-214-checkout\n')
      return done('git@github.com:kestrel/storefront.git\n')
    }
    switch (args.join(' ')) {
      case 'status --json':
        return done(JSON.stringify(STATUS))
      case 'review --json':
        return done(JSON.stringify(REVIEW))
      case 'report --by ticket --json':
      case 'report --by ticket --json --filter ticket=KD-214 --since 2026-10-06':
        return done(JSON.stringify(REPORT))
      case 'record':
        return done('')
      case 'submit':
        return done('submitted token sheet · 5 token entries · $212.40 · 95% on tickets\n')
      default:
        return done(`tc: unknown command ${args.join(' ')}`, 2)
    }
  })
  on('session.id', () => ({ value: 'session-1' }))
  on('session.cwd', () => ({ value: '/work/storefront' }))
  on('session.usage', () => {
    cost += 0.5
    return {
      value: { startedAt: NOW, context: { window: 200_000 }, rateLimits: [], cost: { usd: cost } },
    }
  })
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
  on('prompt.submit', (_, e) => ({ text: e.text }))
  return { clock, runs, machine }
}

function records(runs: Run[]): Array<Record<string, unknown>> {
  return runs
    .filter(run => run.argv[1] === 'record')
    .flatMap(run => (run.stdin ?? '').split('\n').filter(Boolean))
    .map(line => JSON.parse(line) as Record<string, unknown>)
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
  test('each turn goes to tc with its usage, cost, branch and ticket keys', async ($, on) => {
    const { clock, runs } = world(on)
    await start($)
    await $.prompt.submit({
      text: 'Fix the rounding for KD-214 please',
      origin: { kind: 'composer' },
    } as never)
    await $.turn.complete(TURN)
    await clock.settle()

    const turns = records(runs).filter(r => r.kind === 'turn')
    expect(turns).toEqual([
      {
        kind: 'turn',
        session_id: 'session-1',
        turn_id: 'turn-1',
        at: '2026-10-12T09:00:00.000Z',
        model: 'claude-opus-5',
        tokens: { input: 1200, output: 340, cache_write: 2000, cache_read: 18000 },
        reported_cost: 1.75,
        cwd: '/work/storefront',
        branch: 'kd-214-checkout',
        repository: 'kestrel/storefront',
        ticket_keys: ['KD-214'],
      },
    ])
  })

  test('the summary is written by a small model call and handed to tc', async ($, on) => {
    const { clock, runs } = world(on)
    await start($)
    await $.turn.complete(TURN)
    await clock.settle()

    expect(records(runs).filter(r => r.kind === 'summary')).toEqual([
      {
        kind: 'summary',
        session_id: 'session-1',
        summary: 'Fixed rounding in the checkout total and added tests',
      },
    ])
  })

  test('without tc the records wait on the machine and the band says how to install it', async ($, on) => {
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
    await $.turn.complete({ ...TURN, turnId: 'turn-2' })
    await clock.settle()

    const turns = records(runs).filter(r => r.kind === 'turn')
    expect(turns.map(r => r.turn_id)).toEqual(['turn-1', 'turn-2'])
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
    expect(records(runs).filter(r => r.kind === 'tags')).toEqual([
      { kind: 'tags', session_id: 'session-1', tags: { ticket: 'KD-231' } },
    ])
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

    expect(result.text).toBe('submitted token sheet · 5 token entries · $212.40 · 95% on tickets')
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
    input: { by: 'ticket', filter: 'ticket=KD-214', since: '2026-10-06' },
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
        await band.find({ type: 'Text', text: '$212 spent · 91% on tickets · KD-214' }),
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
      expect(runs.some(run => run.argv.slice(1).join(' ') === 'tag 2 ticket=KD-231')).toBe(true)

      await pane.press({ key: 'submit' })
      expect(runs.some(run => run.argv[1] === 'submit')).toBe(true)
    })
  }
})
