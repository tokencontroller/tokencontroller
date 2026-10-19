/**
 * Reads the signals from the variables every job has and from the event that
 * started it. A review of a pull request lands on that pull request's
 * feature, so the number and the title (where feature keys are found) matter.
 */
export function readJobSignals(env, readEvent) {
    let event = {};
    if (env.GITHUB_EVENT_PATH) {
        try {
            event = JSON.parse(readEvent(env.GITHUB_EVENT_PATH));
        }
        catch {
            event = {};
        }
    }
    // pull_request and pull_request_target carry the pull request; a comment on
    // one (issue_comment) carries it as an issue that has a pull_request field.
    const pr = event.pull_request ??
        (event.issue?.pull_request !== undefined ? event.issue : undefined) ??
        undefined;
    const number = typeof pr?.number === 'number' ? pr.number : undefined;
    const title = typeof pr?.title === 'string' ? pr.title : undefined;
    const headRef = typeof event.pull_request?.head?.ref === 'string' ? event.pull_request.head.ref : undefined;
    return {
        repository: repositoryOf(env.GITHUB_SERVER_URL, env.GITHUB_REPOSITORY),
        branch: env.GITHUB_HEAD_REF || headRef || branchOf(env.GITHUB_REF),
        pull_request: number,
        pull_request_title: title,
    };
}
/** `github.com/owner/name`, the way the token entry schema spells a repository. */
function repositoryOf(server, name) {
    if (!name)
        return undefined;
    const host = server && URL.canParse(server) ? new URL(server).host : 'github.com';
    return `${host}/${name}`;
}
function branchOf(ref) {
    return ref?.startsWith('refs/heads/') ? ref.slice('refs/heads/'.length) : undefined;
}
