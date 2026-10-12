import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseTags, readInputs } from './inputs.ts'

const BASE = { INPUT_TOKEN: 'tc_svc_abc', INPUT_URL: 'https://app.tokencontroller.com' }

test('reads the inputs from the docs example', () => {
  const inputs = readInputs({
    ...BASE,
    INPUT_PROJECT: 'storefront',
    INPUT_TICKET: 'KD-214',
    INPUT_TAGS: 'purpose=code-review',
  })
  assert.deepEqual(inputs, {
    token: 'tc_svc_abc',
    url: 'https://app.tokencontroller.com',
    project: 'storefront',
    ticket: 'KD-214',
    tags: { purpose: 'code-review' },
  })
})

test('takes TC_TICKET from an earlier step when no ticket is given', () => {
  assert.equal(readInputs({ ...BASE, TC_TICKET: 'KD-231' }).ticket, 'KD-231')
  assert.equal(readInputs({ ...BASE, INPUT_TICKET: 'KD-1', TC_TICKET: 'KD-231' }).ticket, 'KD-1')
  assert.equal(readInputs({ ...BASE, INPUT_TICKET: '', TC_TICKET: ' ' }).ticket, undefined)
})

test('a project or ticket in tags counts as the input', () => {
  const inputs = readInputs({ ...BASE, INPUT_TAGS: 'project=storefront ticket=KD-9 team=web' })
  assert.equal(inputs.project, 'storefront')
  assert.equal(inputs.ticket, 'KD-9')
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
