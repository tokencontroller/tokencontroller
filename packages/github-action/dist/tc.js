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
/**
 * The tc call that reads this job's session files and reports them. Tags
 * named here win over every rule (docs: Collect, GitHub Action).
 */
export function submitArgs(inputs, signals) {
    const args = ['submit'];
    const tags = [
        ['project', inputs.project],
        ['ticket', inputs.ticket],
        ...Object.entries(inputs.tags),
    ];
    for (const [name, value] of tags) {
        if (value !== undefined)
            args.push('--tag', `${name}=${value}`);
    }
    const found = [
        ['repository', signals.repository],
        ['branch', signals.branch],
        ['pull_request', signals.pull_request],
        ['pull_request_title', signals.pull_request_title],
    ];
    for (const [name, value] of found) {
        if (value !== undefined)
            args.push('--signal', `${name}=${value}`);
    }
    return args;
}
/** How many token entries tc said it reported, from its last line. */
export function reportedCount(output) {
    const match = /(\d+) token entr(?:y|ies)/.exec(output);
    return match?.[1] === undefined ? undefined : Number(match[1]);
}
/** The environment tc runs with: the token and address, and nothing to ask. */
export function tcEnv(inputs, base) {
    return { ...base, TC_TOKEN: inputs.token, TC_URL: inputs.url, NO_COLOR: '1' };
}
