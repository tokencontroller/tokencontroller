import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseTags, readInputs } from './inputs.ts'

const BASE = { INPUT_TOKEN: 'tc_svc_abc', INPUT_URL: 'https://app.tokencontroller.com' }

test('reads the inputs from the docs example', () => {
  const inputs = readInputs({
    ...BASE,
    INPUT_PROJECT: 'shop',
    INPUT_FEATURE: 'KD-214',
    INPUT_TAGS: 'purpose=code-review',
  })
  assert.deepEqual(inputs, {
    token: 'tc_svc_abc',
    url: 'https://app.tokencontroller.com',
    project: 'shop',
    feature: 'KD-214',
    tags: { purpose: 'code-review' },
  })
})

test('takes TC_FEATURE from an earlier step when no feature is given', () => {
  assert.equal(readInputs({ ...BASE, TC_FEATURE: 'KD-231' }).feature, 'KD-231')
  assert.equal(readInputs({ ...BASE, INPUT_FEATURE: 'KD-1', TC_FEATURE: 'KD-231' }).feature, 'KD-1')
  assert.equal(readInputs({ ...BASE, INPUT_FEATURE: '', TC_FEATURE: ' ' }).feature, undefined)
})

test('a project or feature in tags counts as the input', () => {
  const inputs = readInputs({ ...BASE, INPUT_TAGS: 'project=shop feature=KD-9 team=web' })
  assert.equal(inputs.project, 'shop')
  assert.equal(inputs.feature, 'KD-9')
  assert.deepEqual(inputs.tags, { team: 'web' })
})

test('a self-hosted address loses its trailing slash', () => {
  assert.equal(
    readInputs({ ...BASE, INPUT_URL: 'https://tc.acme.test/' }).url,
    'https://tc.acme.test',
  )
})

test('refuses a missing or wrong token', () => {
  assert.throws(() => readInputs({}), /token input is empty/)
  assert.throws(() => readInputs({ INPUT_TOKEN: 'tc_org_abc' }), /not a service token/)
})

test('tags are split by commas, spaces and new lines', () => {
  assert.deepEqual(parseTags('purpose=code-review, team=web\nclient=acme'), {
    purpose: 'code-review',
    team: 'web',
    client: 'acme',
  })
  assert.deepEqual(parseTags(''), {})
})

test('a tag has one value', () => {
  assert.throws(() => parseTags('team=web team=api'), /gives team twice/)
  assert.throws(() => parseTags('team'), /"team", which is not name=value/)
  assert.throws(() => parseTags('=web'), /not name=value/)
})
