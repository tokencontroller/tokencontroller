import assert from 'node:assert/strict'
import { test } from 'node:test'

import { assetFor, checksumOf, releaseUrl, reportedCount, submitArgs } from './tc.ts'

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

test('passes tags and job signals to tc submit', () => {
  const args = submitArgs(
    {
      token: 'tc_svc_abc',
      url: 'https://app.tokencontroller.com',
      project: 'storefront',
      ticket: 'KD-214',
      tags: { purpose: 'code-review' },
    },
    {
      repository: 'kestrel/storefront',
      branch: 'kd-214-checkout',
      pull_request: 88,
      pull_request_title: 'KD-214 Faster checkout',
    },
  )
  assert.deepEqual(args, [
    'submit',
    '--tag',
    'project=storefront',
    '--tag',
    'ticket=KD-214',
    '--tag',
    'purpose=code-review',
    '--signal',
    'repository=kestrel/storefront',
    '--signal',
    'branch=kd-214-checkout',
    '--signal',
    'pull_request=88',
    '--signal',
    'pull_request_title=KD-214 Faster checkout',
  ])
})

test('leaves out what the job does not know', () => {
  const args = submitArgs(
    { token: 't', url: 'u', project: undefined, ticket: undefined, tags: {} },
    {
      repository: undefined,
      branch: 'main',
      pull_request: undefined,
      pull_request_title: undefined,
    },
  )
  assert.deepEqual(args, ['submit', '--signal', 'branch=main'])
})

test('reads how many token entries tc reported', () => {
  assert.equal(reportedCount('submitted · 3 token entries · $4.10'), 3)
  assert.equal(reportedCount('submitted · 1 token entry · $0.20'), 1)
  assert.equal(reportedCount('nothing to submit'), undefined)
})
