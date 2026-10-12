import { execFile } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { promisify } from 'node:util';
import { readJobSignals } from './github.js';
import { readInputs } from './inputs.js';
import { installTc } from './install.js';
import { readSubmitted, SUBMIT_ARGS, tcEnv } from './tc.js';
const run = promisify(execFile);
async function main() {
    const env = process.env;
    const inputs = readInputs(env);
    console.log(`::add-mask::${inputs.token}`);
    const signals = readJobSignals(env, path => readFileSync(path, 'utf8'));
    const tc = await installTc(env.RUNNER_TOOL_CACHE ?? tmpdir());
    console.log(`tc ${SUBMIT_ARGS.join(' ')}`);
    const { stdout, stderr } = await run(tc, [...SUBMIT_ARGS], {
        env: tcEnv(inputs, signals, env),
        maxBuffer: 16 * 1024 * 1024,
    });
    if (stderr.trim() !== '')
        console.error(stderr.trim());
    const done = readSubmitted(stdout);
    const entries = `${done.count} token ${done.count === 1 ? 'entry' : 'entries'}`;
    const where = [inputs.ticket, inputs.project].filter(Boolean).join(' · ');
    const value = done.listValue === undefined ? '' : `, ${done.listValue} ${done.currency ?? ''}`;
    const line = `Reported ${entries}${where ? ` on ${where}` : ''}${value.trimEnd()}.`;
    console.log(line);
    if (env.GITHUB_OUTPUT)
        appendFileSync(env.GITHUB_OUTPUT, `token-entries=${done.count}\n`);
    if (env.GITHUB_STEP_SUMMARY) {
        appendFileSync(env.GITHUB_STEP_SUMMARY, `### Token Controller\n\n${line}\n`);
    }
}
main().catch((error) => {
    const failed = error;
    const why = failed.stderr?.trim() || failed.message || String(error);
    // One line per message, as workflow commands need.
    console.log(`::error title=Token Controller::${why.replace(/\r?\n/g, '%0A')}`);
    process.exitCode = 1;
});
