import assert from 'node:assert/strict'
import { test } from 'node:test'

import { cleanSummary, summaryPrompt } from './summary.ts'

test('the prompt carries the summary so far and the latest messages', () => {
  const prompt = summaryPrompt('Started the checkout fix', [
    { role: 'user', text: 'Now add tests' },
    { role: 'assistant', text: '' },
    { role: 'assistant', text: 'Added three tests.' },
  ])
  assert.match(prompt, /^The summary so far: Started the checkout fix/)
  assert.match(prompt, /Person: Now add tests\nAgent: Added three tests\./)
})

test('long messages are cut and the oldest dropped first', () => {
  const long = 'x'.repeat(5000)
  const messages = Array.from({ length: 20 }, (_, i) => ({
    role: 'user' as const,
    text: `${i} ${long}`,
  }))
  const prompt = summaryPrompt(null, messages)
  assert.ok(prompt.length < 13_000)
  assert.match(prompt, /Person: 19 x+…/)
  assert.doesNotMatch(prompt, /Person: 0 /)
})

test('the answer becomes one clean line', () => {
  assert.equal(cleanSummary('\n"Fixed rounding in checkout."\nMore'), 'Fixed rounding in checkout')
  assert.equal(cleanSummary('   '), null)
  assert.equal(cleanSummary('y'.repeat(300))?.length, 200)
})
