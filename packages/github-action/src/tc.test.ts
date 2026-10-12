import assert from 'node:assert/strict'
import { test } from 'node:test'

import { assetFor, checksumOf, readSubmitted, releaseUrl, tcEnv } from './tc.ts'

test('picks the release asset for each runner', () => {
  assert.equal(assetFor('linux', 'x64'), 'tc-x86_64-unknown-linux-gnu.tar.gz')
  assert.equal(assetFor('linux', 'arm64'), 'tc-aarch64-unknown-linux-gnu.tar.gz')
  assert.equal(assetFor('darwin', 'arm64'), 'tc-aarch64-apple-darwin.tar.gz')
  assert.equal(assetFor('win32', 'x64'), 'tc-x86_64-pc-windows-msvc.zip')
  assert.throws(() => assetFor('freebsd', 'x64'), /no build for freebsd/)
})

test('downloads from the 1.0.0 release', () => {
  assert.equal(
    releaseUrl('SHA256SUMS'),
    'https://github.com/tokencontroller/tokencontroller/releases/download/v1.0.0/SHA256SUMS',
  )
})

test('finds a checksum in SHA256SUMS', () => {
  const sum = 'a'.repeat(64)
  const listing = `${'b'.repeat(64)}  tc-x86_64-apple-darwin.tar.gz\n${sum} *tc-x86_64-unknown-linux-gnu.tar.gz\n`
  assert.equal(checksumOf(listing, 'tc-x86_64-unknown-linux-gnu.tar.gz'), sum)
  assert.equal(checksumOf(listing, 'tc-aarch64-pc-windows-msvc.zip'), undefined)
})

const INPUTS = {
  token: 'tc_svc_abc',
  url: 'https://app.tokencontroller.com',
  project: 'storefront',
  ticket: 'KD-214',
  tags: { purpose: 'code-review', team: 'web' },
}

const SIGNALS = {
  repository: 'github.com/kestrel/storefront',
  branch: 'kd-214-checkout',
  pull_request: 88,
  pull_request_title: 'KD-214 Faster checkout',
}

test('hands the token, the tags and the job signals to tc through its environment', () => {
  const env = tcEnv(INPUTS, SIGNALS, { PATH: '/usr/bin', TC_TOKEN: 'old' })
  assert.deepEqual(env, {
    PATH: '/usr/bin',
    TC_TOKEN: 'tc_svc_abc',
    TC_URL: 'https://app.tokencontroller.com',
    NO_COLOR: '1',
    TC_PROJECT: 'storefront',
    TC_TICKET: 'KD-214',
    TC_TAGS: 'purpose=code-review,team=web',
    TC_SIGNALS:
      '{"repository":"github.com/kestrel/storefront","branch":"kd-214-checkout","pull_request":88,"pull_request_title":"KD-214 Faster checkout"}',
  })
})

test('leaves out what the job does not know, and keeps what an earlier step set', () => {
  const env = tcEnv(
    { token: 't', url: 'u', project: undefined, ticket: undefined, tags: {} },
    {
      repository: undefined,
      branch: 'main',
      pull_request: undefined,
      pull_request_title: undefined,
    },
    { TC_PROJECT: 'from-an-earlier-step' },
  )
  assert.equal(env.TC_PROJECT, 'from-an-earlier-step')
  assert.equal(env.TC_TICKET, undefined)
  assert.equal(env.TC_TAGS, undefined)
  assert.equal(env.TC_SIGNALS, '{"branch":"main"}')
})

test('reads what tc submit reported', () => {
  assert.deepEqual(
    readSubmitted(
      '{"submitted":3,"list_value":"4.10","currency":"USD","share_on_tickets_percent":100}',
    ),
    { count: 3, listValue: '4.10', currency: 'USD' },
  )
  assert.deepEqual(readSubmitted('{}'), { count: 0, listValue: undefined, currency: undefined })
  assert.throws(() => readSubmitted('submitted 3'), /something other than JSON/)
})
