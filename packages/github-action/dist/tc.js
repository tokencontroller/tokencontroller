/** The tc release this Action installs: the one released with it. */
export const TC_VERSION = '1.0.0';
const RELEASES = 'https://github.com/tokencontroller/tokencontroller/releases/download';
/** The release asset for this runner, by Rust target triple. */
export function assetFor(platform, arch) {
    const cpu = arch === 'x64' ? 'x86_64' : arch === 'arm64' ? 'aarch64' : undefined;
    const system = platform === 'linux'
        ? 'unknown-linux-gnu'
        : platform === 'darwin'
            ? 'apple-darwin'
            : platform === 'win32'
                ? 'pc-windows-msvc'
                : undefined;
    if (cpu === undefined || system === undefined) {
        throw new Error(`tc has no build for ${platform} on ${arch}. Use a Linux, macOS or Windows runner.`);
    }
    return `tc-${cpu}-${system}.${platform === 'win32' ? 'zip' : 'tar.gz'}`;
}
export function releaseUrl(file, version = TC_VERSION) {
    return `${RELEASES}/v${version}/${file}`;
}
/** Finds the checksum of `file` in a SHA256SUMS listing. */
export function checksumOf(listing, file) {
    for (const line of listing.split('\n')) {
        const [sum, name] = line.trim().split(/\s+\*?/);
        if (name === file && sum !== undefined && /^[0-9a-f]{64}$/i.test(sum))
            return sum.toLowerCase();
    }
    return undefined;
}
/** The tc call that reads this job's session files and reports them. */
export const SUBMIT_ARGS = ['submit', '--json'];
/**
 * The environment tc runs with. The token and address, then the project,
 * ticket and tags that win over every rule (docs: Collect, GitHub Action),
 * and the job's signals, so a review lands on its pull request's ticket. The
 * pull request title only goes to tc, which keeps the ticket keys in it.
 */
export function tcEnv(inputs, signals, base) {
    const env = {
        ...base,
        TC_TOKEN: inputs.token,
        TC_URL: inputs.url,
        NO_COLOR: '1',
    };
    if (inputs.project !== undefined)
        env.TC_PROJECT = inputs.project;
    if (inputs.ticket !== undefined)
        env.TC_TICKET = inputs.ticket;
    const tags = Object.entries(inputs.tags);
    if (tags.length > 0)
        env.TC_TAGS = tags.map(([name, value]) => `${name}=${value}`).join(',');
    const known = Object.fromEntries(Object.entries(signals).filter(([, v]) => v !== undefined));
    if (Object.keys(known).length > 0)
        env.TC_SIGNALS = JSON.stringify(known);
    return env;
}
export function readSubmitted(stdout) {
    let parsed;
    try {
        parsed = JSON.parse(stdout);
    }
    catch {
        throw new Error(`tc answered with something other than JSON: ${stdout.trim().slice(0, 200)}`);
    }
    const top = (typeof parsed === 'object' && parsed !== null ? parsed : {});
    return {
        count: typeof top.submitted === 'number' ? top.submitted : 0,
        listValue: typeof top.list_value === 'string' ? top.list_value : undefined,
        currency: typeof top.currency === 'string' ? top.currency : undefined,
    };
}
