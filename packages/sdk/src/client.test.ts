import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { TokenController, TokenControllerError } from './client.ts'
import type { TokenEntry } from './token-entry.ts'

const ENTRY: TokenEntry = {
  agent_harness: 'support-desk',
  agent_name: 'email answers',
  model: 'claude-sonnet-5',
  started_at: '2026-09-18T09:12:00Z',
  ended_at: '2026-09-18T09:14:10Z',
  tokens: { input: 4120, output: 610, cache_read: 18200 },
  reported_cost: 0.031,
  summary: 'Answered refund question for order 1182',
  project: 'support',
  tags: { client: 'acme', purpose: 'support' },
}

type Call = { url: string; init: RequestInit }

function fakeFetch(...answers: Array<Response | Error>) {
  const calls: Call[] = []
  const fetch = async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), init: init ?? {} })
    const answer = answers.shift()
    if (answer === undefined) throw new Error('no more answers')
    if (answer instanceof Error) throw answer
    return answer
  }
  return { calls, fetch: fetch as typeof globalThis.fetch }
}

const created = () => Response.json({ id: 'te_1', status: 'submitted' }, { status: 201 })

describe('tokenEntries.create', () => {
  test('posts the entry with the service token to the hosted service', async () => {
    const { calls, fetch } = fakeFetch(created())
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    const answer = await tc.tokenEntries.create(ENTRY)

    assert.equal(answer.id, 'te_1')
    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.url, 'https://app.tokencontroller.com/api/v1/token-entries')
    assert.equal(calls[0]?.init.method, 'POST')
    const headers = calls[0]?.init.headers as Record<string, string>
    assert.equal(headers.authorization, 'Bearer tc_svc_abc')
    assert.deepEqual(JSON.parse(String(calls[0]?.init.body)), ENTRY)
  })

  test('uses a self-hosted address without a trailing slash', async () => {
    const { calls, fetch } = fakeFetch(created())
    const tc = new TokenController({ token: 'tc_svc_abc', url: 'https://tc.acme.test/', fetch })

    await tc.tokenEntries.create(ENTRY)

    assert.equal(calls[0]?.url, 'https://tc.acme.test/api/v1/token-entries')
  })

  test('refuses a token that is not a service token', () => {
    assert.throws(
      () => new TokenController({ token: 'tc_read_abc' }),
      (error: unknown) => error instanceof TokenControllerError && /tc_svc_/.test(error.message),
    )
  })

  test('refuses a broken entry before sending it', async () => {
    const { calls, fetch } = fakeFetch()
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    await assert.rejects(
      tc.tokenEntries.create({ ...ENTRY, model: '', tokens: { input: -1 } }),
      /model is required; tokens.input must be a whole number/,
    )
    assert.equal(calls.length, 0)
  })

  test('says what the service refused', async () => {
    const { fetch } = fakeFetch(
      Response.json({ message: 'unknown tag name: cleint' }, { status: 422 }),
    )
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    await assert.rejects(tc.tokenEntries.create(ENTRY), (error: unknown) => {
      assert.ok(error instanceof TokenControllerError)
      assert.equal(error.status, 422)
      assert.match(error.message, /unknown tag name: cleint/)
      return true
    })
  })

  test('tries an entry with a session ID again after a server error', async () => {
    const { calls, fetch } = fakeFetch(new Response('busy', { status: 503 }), created())
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    const answer = await tc.tokenEntries.create({ ...ENTRY, session_id: 's-1' })

    assert.equal(answer.id, 'te_1')
    assert.equal(calls.length, 2)
  })

  test('sends an entry without a session ID only once', async () => {
    const { calls, fetch } = fakeFetch(new Response('busy', { status: 503 }), created())
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    await assert.rejects(tc.tokenEntries.create(ENTRY), /answered 503: busy/)
    assert.equal(calls.length, 1)
  })

  test('does not retry a refusal', async () => {
    const { calls, fetch } = fakeFetch(new Response('no', { status: 401 }), created())
    const tc = new TokenController({ token: 'tc_svc_abc', fetch })

    await assert.rejects(tc.tokenEntries.create({ ...ENTRY, session_id: 's-1' }), /401/)
    assert.equal(calls.length, 1)
  })

  test('says when the service cannot be reached', async () => {
    const { fetch } = fakeFetch(new Error('getaddrinfo ENOTFOUND'))
    const tc = new TokenController({ token: 'tc_svc_abc', retries: 0, fetch })

    await assert.rejects(
      tc.tokenEntries.create({ ...ENTRY, session_id: 's-1' }),
      /Could not reach https:\/\/app.tokencontroller.com: getaddrinfo ENOTFOUND/,
    )
  })
})
