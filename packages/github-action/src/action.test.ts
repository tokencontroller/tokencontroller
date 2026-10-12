import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { chmod, mkdtemp, readdir, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { delimiter, join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const run = promisify(execFile)
const here = fileURLToPath(new URL('.', import.meta.url))
const root = join(here, '..')

/** A stand-in tc on the PATH that prints how it was called. */
async function fakeTc(folder: string): Promise<void> {
  const script = [
    '#!/bin/sh',
    'if [ "$1" = "--version" ]; then echo "tc 1.0.0"; exit 0; fi',
    'echo "args: $*" >&2',
    'echo "token: $TC_TOKEN url: $TC_URL" >&2',
    'echo "project: $TC_PROJECT ticket: $TC_TICKET tags: $TC_TAGS" >&2',
    'echo "signals: $TC_SIGNALS" >&2',
    'echo \'{"submitted":2,"list_value":"3.20","currency":"USD","share_on_tickets_percent":100}\'',
  ].join('\n')
  await writeFile(join(folder, 'tc'), script)
  await chmod(join(folder, 'tc'), 0o755)
}

test('the action runs tc submit with the inputs and reports the count', {
  skip: process.platform === 'win32',
}, async () => {
  const folder = await mkdtemp(join(tmpdir(), 'tc-action-'))
  await fakeTc(folder)
  const output = join(folder, 'output')
  const summary = join(folder, 'summary')
  const event = join(folder, 'event.json')
  await writeFile(output, '')
  await writeFile(summary, '')
  await writeFile(event, JSON.stringify({ pull_request: { number: 88, title: 'KD-214 Checkout' } }))

  const { stdout, stderr } = await run(process.execPath, [join(root, 'src/main.ts')], {
    env: {
      PATH: `${folder}${delimiter}${process.env.PATH}`,
      INPUT_TOKEN: 'tc_svc_secret',
      INPUT_URL: 'https://app.tokencontroller.com',
      INPUT_PROJECT: 'storefront',
      INPUT_TAGS: 'purpose=code-review',
      TC_TICKET: 'KD-214',
      GITHUB_SERVER_URL: 'https://github.com',
      GITHUB_REPOSITORY: 'kestrel/storefront',
      GITHUB_HEAD_REF: 'kd-214-checkout',
      GITHUB_EVENT_PATH: event,
      GITHUB_OUTPUT: output,
      GITHUB_STEP_SUMMARY: summary,
      RUNNER_TOOL_CACHE: folder,
    },
  })

  assert.match(stdout, /^::add-mask::tc_svc_secret$/m)
  assert.match(stderr, /^args: submit --json$/m)
  assert.match(stderr, /token: tc_svc_secret url: https:\/\/app.tokencontroller.com/)
  assert.match(stderr, /project: storefront ticket: KD-214 tags: purpose=code-review/)
  assert.match(
    stderr,
    /signals: \{"repository":"github.com\/kestrel\/storefront","branch":"kd-214-checkout","pull_request":88,"pull_request_title":"KD-214 Checkout"\}/,
  )
  assert.equal(await readFile(output, 'utf8'), 'token-entries=2\n')
  assert.match(
    await readFile(summary, 'utf8'),
    /Reported 2 token entries on KD-214 · storefront, 3.20 USD\./,
  )
})

test('the action fails with a plain error when the token is missing', async () => {
  await assert.rejects(
    run(process.execPath, [join(root, 'src/main.ts')], { env: { PATH: process.env.PATH } }),
    (error: { stdout: string; code: number }) => {
      assert.equal(error.code, 1)
      assert.match(error.stdout, /^::error title=Token Controller::The token input is empty/m)
      return true
    },
  )
})

test('dist holds the build of src, as GitHub runs dist', async () => {
  const fresh = await mkdtemp(join(tmpdir(), 'tc-action-dist-'))
  await run(
    process.execPath,
    [
      join(root, 'node_modules/typescript/bin/tsc'),
      '-p',
      join(root, 'tsconfig.build.json'),
      '--outDir',
      fresh,
    ],
    { cwd: root },
  )
  const built = (await readdir(fresh)).sort()
  const committed = (await readdir(join(root, 'dist'))).sort()
  assert.deepEqual(committed, built, 'run pnpm build in packages/github-action and commit dist')
  for (const file of built) {
    assert.equal(
      await readFile(join(root, 'dist', file), 'utf8'),
      await readFile(join(fresh, file), 'utf8'),
      `dist/${file} is out of date: run pnpm build in packages/github-action and commit dist`,
    )
  }
})
