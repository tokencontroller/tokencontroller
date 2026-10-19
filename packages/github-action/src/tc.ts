import type { JobSignals } from './github.ts'
import type { Inputs } from './inputs.ts'

/** The tc release this Action installs: the one released with it. */
export const TC_VERSION = '1.0.0'

const RELEASES = 'https://github.com/tokencontroller/tokencontroller/releases/download'

/** The release asset for this runner, by Rust target triple. */
export function assetFor(platform: string, arch: string): string {
  const cpu = arch === 'x64' ? 'x86_64' : arch === 'arm64' ? 'aarch64' : undefined
  const system =
    platform === 'linux'
      ? 'unknown-linux-gnu'
      : platform === 'darwin'
        ? 'apple-darwin'
        : platform === 'win32'
          ? 'pc-windows-msvc'
          : undefined
  if (cpu === undefined || system === undefined) {
    throw new Error(
      `tc has no build for ${platform} on ${arch}. Use a Linux, macOS or Windows runner.`,
    )
  }
  return `tc-${cpu}-${system}.${platform === 'win32' ? 'zip' : 'tar.gz'}`
}

export function releaseUrl(file: string, version = TC_VERSION): string {
  return `${RELEASES}/v${version}/${file}`
}

/** Finds the checksum of `file` in a SHA256SUMS listing. */
export function checksumOf(listing: string, file: string): string | undefined {
  for (const line of listing.split('\n')) {
    const [sum, name] = line.trim().split(/\s+\*?/)
    if (name === file && sum !== undefined && /^[0-9a-f]{64}$/i.test(sum)) return sum.toLowerCase()
  }
  return undefined
}

/** The tc call that reads this job's session files and reports them. */
export const SUBMIT_ARGS = ['submit', '--json'] as const

/**
 * The environment tc runs with. The token and address, then the project,
 * feature and tags that win over every rule (docs: Collect, GitHub Action),
 * and the job's signals, so a review lands on its pull request's feature. The
 * pull request title only goes to tc, which keeps the feature keys in it.
 */
export function tcEnv(
  inputs: Inputs,
  signals: JobSignals,
  base: Record<string, string | undefined>,
): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = {
    ...base,
    TC_TOKEN: inputs.token,
    TC_URL: inputs.url,
    NO_COLOR: '1',
  }
  if (inputs.project !== undefined) env.TC_PROJECT = inputs.project
  if (inputs.feature !== undefined) env.TC_FEATURE = inputs.feature
  const tags = Object.entries(inputs.tags)
  if (tags.length > 0) env.TC_TAGS = tags.map(([name, value]) => `${name}=${value}`).join(',')
  const known = Object.fromEntries(Object.entries(signals).filter(([, v]) => v !== undefined))
  if (Object.keys(known).length > 0) env.TC_SIGNALS = JSON.stringify(known)
  return env
}

/** What `tc submit --json` reported: how many messages, and their cost from the price table. */
export interface Submitted {
  count: number
  cost: string | undefined
  currency: string | undefined
}

export function readSubmitted(stdout: string): Submitted {
  let parsed: unknown
  try {
    parsed = JSON.parse(stdout)
  } catch {
    throw new Error(`tc answered with something other than JSON: ${stdout.trim().slice(0, 200)}`)
  }
  const top = (typeof parsed === 'object' && parsed !== null ? parsed : {}) as Record<
    string,
    unknown
  >
  return {
    count: typeof top.submitted === 'number' ? top.submitted : 0,
    cost: typeof top.cost === 'string' ? top.cost : undefined,
    currency: typeof top.currency === 'string' ? top.currency : undefined,
  }
}
