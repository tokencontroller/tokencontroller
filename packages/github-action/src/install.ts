import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, readdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { promisify } from 'node:util'

import { assetFor, checksumOf, releaseUrl, TC_VERSION } from './tc.ts'

const run = promisify(execFile)

/**
 * Puts the tc binary for this runner in the tool cache and returns its path.
 * A tc already on the PATH is used as it is.
 */
export async function installTc(toolCache: string): Promise<string> {
  const found = await onPath()
  if (found) return 'tc'

  const asset = assetFor(process.platform, process.arch)
  const folder = join(toolCache, 'tokencontroller-tc', TC_VERSION, process.arch)
  const binary = join(folder, process.platform === 'win32' ? 'tc.exe' : 'tc')
  const existing: string[] = await readdir(folder).catch(() => [])
  if (existing.includes(process.platform === 'win32' ? 'tc.exe' : 'tc')) return binary

  const [archive, sums] = await Promise.all([
    download(releaseUrl(asset)),
    download(releaseUrl('SHA256SUMS')),
  ])
  const expected = checksumOf(sums.toString('utf8'), asset)
  const actual = createHash('sha256').update(archive).digest('hex')
  if (expected === undefined)
    throw new Error(`SHA256SUMS of tc ${TC_VERSION} does not list ${asset}.`)
  if (expected !== actual) throw new Error(`${asset} does not match its checksum. Not running it.`)

  await mkdir(folder, { recursive: true })
  const file = join(folder, asset)
  await writeFile(file, archive)
  // tar on every runner image reads .tar.gz, and bsdtar on Windows reads .zip.
  await run('tar', ['-xf', file, '-C', folder])
  return binary
}

async function onPath(): Promise<boolean> {
  try {
    await run('tc', ['--version'])
    return true
  } catch {
    return false
  }
}

async function download(url: string): Promise<Buffer> {
  const response = await fetch(url, { redirect: 'follow', signal: AbortSignal.timeout(120_000) })
  if (!response.ok) throw new Error(`Could not download ${url}: ${response.status}`)
  return Buffer.from(await response.arrayBuffer())
}
