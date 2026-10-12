/**
 * The session's summary: one line on what the session did, written during the
 * session by a small model call, so it exists even if the session is closed
 * abruptly. It is the only text of a token entry besides the tag values, so
 * it names the work and never quotes code, secrets or customer data.
 */

export const SUMMARY_MODEL = 'haiku'

export const SUMMARY_SYSTEM = [
  'You write the one-line summary of a coding agent session for a cost report.',
  'Say what the work was, in plain words, as a person would log it on a timesheet:',
  'for example "Fixed rounding in the checkout total and added tests" or',
  '"Reviewed pull request 88 for the search page".',
  'One sentence, at most 120 characters, no quotes, no trailing period.',
  'Never include code, file contents, secrets, names of customers or personal data.',
  'Answer with the sentence alone.',
].join(' ')

export interface Message {
  role: 'user' | 'assistant'
  text: string
}

const PER_MESSAGE = 1500
const IN_ALL = 12_000

/** The prompt: the summary so far and the latest messages, newest last. */
export function summaryPrompt(previous: string | null, messages: readonly Message[]): string {
  const picked: string[] = []
  let room = IN_ALL
  for (const message of [...messages].reverse()) {
    const text = message.text.trim()
    if (text === '') continue
    const cut = text.length > PER_MESSAGE ? `${text.slice(0, PER_MESSAGE)}…` : text
    const line = `${message.role === 'user' ? 'Person' : 'Agent'}: ${cut}`
    if (line.length > room) break
    picked.unshift(line)
    room -= line.length
  }
  return [
    previous ? `The summary so far: ${previous}` : 'There is no summary yet.',
    '',
    'The latest messages of the session:',
    ...picked,
    '',
    'Write the summary of the whole session so far.',
  ].join('\n')
}

/** The model's answer as one clean line, or null when it gave none. */
export function cleanSummary(answer: string): string | null {
  const line = answer
    .split('\n')
    .map(part => part.trim())
    .find(part => part !== '')
  if (line === undefined) return null
  const bare = line.replace(/^["'“”]+|["'“”]+$/g, '').replace(/\.$/, '')
  return bare.length > 200 ? `${bare.slice(0, 199)}…` : bare
}
