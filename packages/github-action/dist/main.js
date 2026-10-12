import { execFile } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { promisify } from 'node:util';
import { readJobSignals } from './github.js';
import { readInputs } from './inputs.js';
import { installTc } from './install.js';
import { reportedCount, submitArgs, tcEnv } from './tc.js';
const run = promisify(execFile);
async function main() {
    const env = process.env;
    const inputs = readInputs(env);
    console.log(`::add-mask::${inputs.token}`);
    const signals = readJobSignals(env, path => readFileSync(path, 'utf8'));
    const tc = await installTc(env.RUNNER_TOOL_CACHE ?? tmpdir());
    const args = submitArgs(inputs, signals);
    console.log(`tc ${args.join(' ')}`);
    const { stdout, stderr } = await run(tc, args, {
        env: tcEnv(inputs, env),
        maxBuffer: 16 * 1024 * 1024,
    });
    if (stderr.trim() !== '')
        console.error(stderr.trim());
    console.log(stdout.trim());
    const count = reportedCount(stdout) ?? 0;
    if (env.GITHUB_OUTPUT)
        appendFileSync(env.GITHUB_OUTPUT, `token-entries=${count}\n`);
    if (env.GITHUB_STEP_SUMMARY) {
        const where = [inputs.ticket, inputs.project].filter(Boolean).join(' · ');
        appendFileSync(env.GITHUB_STEP_SUMMARY, `### Token Controller\n\nReported ${count} token ${count === 1 ? 'entry' : 'entries'}${where ? ` on ${where}` : ''}.\n`);
    }
}
main().catch((error) => {
    const failed = error;
    const why = failed.stderr?.trim() || failed.message || String(error);
    // One line per message, as workflow commands need.
    console.log(`::error title=Token Controller::${why.replace(/\r?\n/g, '%0A')}`);
    process.exitCode = 1;
});
