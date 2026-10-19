/** What the workflow passed to the step, read from the INPUT_* variables. */
export interface Inputs {
  token: string
  url: string
  project: string | undefined
  feature: string | undefined
  tags: Record<string, string>
}

type Env = Record<string, string | undefined>

/** GitHub turns `with: { feature: … }` into INPUT_FEATURE. */
function input(env: Env, name: string): string | undefined {
  const value = env[`INPUT_${name.replace(/ /g, '_').toUpperCase()}`]?.trim()
  return value === undefined || value === '' ? undefined : value
}

export function readInputs(env: Env): Inputs {
  const token = input(env, 'token')
  if (token === undefined) {
    throw new Error(
      'The token input is empty. Create a service token in the web app under Settings, store it as a secret, and pass it as token.',
    )
  }
  if (!token.startsWith('tc_svc_')) {
    throw new Error('The token input is not a service token. Service tokens start with tc_svc_.')
  }
  const tags = parseTags(input(env, 'tags') ?? '')
  return {
    token,
    url: (input(env, 'url') ?? 'https://app.tokencontroller.com').replace(/\/+$/, ''),
    project: input(env, 'project') ?? tags.project,
    // The docs promise that TC_FEATURE set by an earlier step works too.
    feature: input(env, 'feature') ?? (env.TC_FEATURE?.trim() || undefined) ?? tags.feature,
    tags: Object.fromEntries(
      Object.entries(tags).filter(([name]) => name !== 'project' && name !== 'feature'),
    ),
  }
}

/**
 * Reads `purpose=code-review, team=web` (commas, spaces or new lines between
 * pairs) into one value per tag name.
 */
export function parseTags(text: string): Record<string, string> {
  const tags: Record<string, string> = {}
  for (const pair of text.split(/[\s,]+/).filter(Boolean)) {
    const at = pair.indexOf('=')
    const name = at > 0 ? pair.slice(0, at).trim() : ''
    const value = at > 0 ? pair.slice(at + 1).trim() : ''
    if (name === '' || value === '') {
      throw new Error(`The tags input has "${pair}", which is not name=value.`)
    }
    if (name in tags && tags[name] !== value) {
      throw new Error(`The tags input gives ${name} twice. A tag has one value.`)
    }
    tags[name] = value
  }
  return tags
}
