import assert from 'node:assert/strict'
import { test } from 'node:test'

import { readJobSignals } from './github.ts'

const event = (body: unknown) => () => JSON.stringify(body)

test('a pull request job gives the number, title and head branch', () => {
  const signals = readJobSignals(
    {
      GITHUB_REPOSITORY: 'kestrel/storefront',
      GITHUB_HEAD_REF: 'kd-214-checkout',
      GITHUB_REF: 'refs/pull/88/merge',
      GITHUB_EVENT_PATH: '/event.json',
    },
    event({ pull_request: { number: 88, title: 'KD-214 Faster checkout', head: { ref: 'x' } } }),
  )
  assert.deepEqual(signals, {
    repository: 'kestrel/storefront',
    branch: 'kd-214-checkout',
    pull_request: 88,
    pull_request_title: 'KD-214 Faster checkout',
  })
})

test('a comment on a pull request gives its number and title', () => {
  const signals = readJobSignals(
    { GITHUB_EVENT_PATH: '/event.json', GITHUB_REF: 'refs/heads/main' },
    event({ issue: { number: 12, title: 'OPS-3 Fix deploy', pull_request: { url: 'x' } } }),
  )
  assert.equal(signals.pull_request, 12)
  assert.equal(signals.pull_request_title, 'OPS-3 Fix deploy')
  assert.equal(signals.branch, 'main')
})

test('a comment on an issue is no pull request', () => {
  const signals = readJobSignals(
    { GITHUB_EVENT_PATH: '/event.json' },
    event({ issue: { number: 12, title: 'Bug' } }),
  )
  assert.equal(signals.pull_request, undefined)
})

test('a push or schedule job gives the branch alone', () => {
  const signals = readJobSignals({ GITHUB_REF: 'refs/heads/nightly-deps' }, () => {
    throw new Error('never read')
  })
  assert.deepEqual(signals, {
    repository: undefined,
    branch: 'nightly-deps',
    pull_request: undefined,
    pull_request_title: undefined,
  })
})

test('an unreadable event file is ignored', () => {
  const signals = readJobSignals({ GITHUB_EVENT_PATH: '/event.json' }, () => 'not json')
  assert.equal(signals.pull_request, undefined)
})
