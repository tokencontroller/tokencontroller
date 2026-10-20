"""Claude reviews every pull request, and blocks the merge only for a bug.

Run by the review job (.github/workflows/review.yml here, the "Claude review"
job in ci.yml in the public repo) in two steps around the
anthropics/claude-code-action step:

  before  Decides whether a review runs. It writes `run=true` and the diff
          to review, or `run=false` with `verdict=` set, to GITHUB_OUTPUT.
          No review for a draft, for the label `review-waived`, when the
          head commit already carries a verdict (a rerun on the same
          commit costs nothing), or when the diff is too large to read.
  after   Reads the review's structured output, posts one summary comment
          on the pull request with a marker that carries the verdict, and
          exits 1 when the verdict is `bug`.

The verdict is `bug` only for a defect Claude confirmed in the changed code:
wrong output, a crash, data loss or a security hole. Everything else is
`clean`, with suggestions as inline comments that block nothing.
"""

import json
import os
import subprocess
import sys

REPO = os.environ["GITHUB_REPOSITORY"]
PR = os.environ["PR"]
SHA = os.environ["HEAD_SHA"]
MARKER = "<!-- claude-review sha={sha} verdict={verdict} -->"
MAX_DIFF = 250_000  # characters; roughly the limit of a careful read
# Generated and vendored files are not reviewed.
EXCLUDED = (
    ":!**/pnpm-lock.yaml",
    ":!**/Cargo.lock",
    ":!**/package-lock.json",
    ":!**/*.snap",
    ":!**/*.min.js",
    ":!**/*.svg",
)


def gh(*args: str) -> str:
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def output(**values: str) -> None:
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as f:
        for key, value in values.items():
            f.write(f"{key}={value}\n")


def comments() -> list[dict]:
    pages = json.loads(gh("api", "--paginate", "--slurp", f"repos/{REPO}/issues/{PR}/comments?per_page=100"))
    return [c for page in pages for c in page]


def earlier_verdict() -> str | None:
    prefix = MARKER.format(sha=SHA, verdict="")
    for comment in comments():
        body = comment.get("body") or ""
        start = body.find(prefix)
        if start >= 0:
            return body[start + len(prefix):].split(" -->", 1)[0].strip()
    return None


def before() -> int:
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8"))
    pr = event["pull_request"]
    labels = {label["name"] for label in pr.get("labels", [])}
    if pr.get("draft"):
        print("Draft pull request: no review until it is ready")
        output(run="false", verdict="")
        return 0
    if "review-waived" in labels:
        print("Label review-waived: the review does not block this pull request")
        output(run="false", verdict="waived")
        return 0
    verdict = earlier_verdict()
    if verdict in {"bug", "clean", "skipped"}:
        print(f"This commit was reviewed already: {verdict}")
        output(run="false", verdict=verdict)
        return 0 if verdict != "bug" else 1
    base = pr["base"]["sha"]
    subprocess.run(["git", "fetch", "--no-tags", "--depth=1", "origin", base], check=True)
    merge_base = subprocess.run(
        ["git", "merge-base", base, SHA], capture_output=True, text=True
    ).stdout.strip() or base
    diff = subprocess.run(
        ["git", "diff", "--no-color", merge_base, SHA, "--", ".", *EXCLUDED],
        check=True, capture_output=True, text=True,
    ).stdout
    if not diff.strip():
        print("Nothing to review after leaving out generated files")
        output(run="false", verdict="clean")
        return 0
    if len(diff) > MAX_DIFF:
        print(f"Diff of {len(diff)} characters is too large for one review; not reviewed")
        post("skipped", f"The change is too large for one review ({len(diff):,} characters of diff). Not reviewed.", [])
        output(run="false", verdict="skipped")
        return 0
    os.makedirs(".claude-review", exist_ok=True)
    with open(".claude-review/diff.patch", "w", encoding="utf-8") as f:
        f.write(diff)
    print(f"Reviewing {len(diff)} characters of diff since {merge_base[:7]}")
    output(run="true", verdict="", merge_base=merge_base)
    return 0


def post(verdict: str, summary: str, bugs: list[dict]) -> None:
    short = SHA[:7]
    lines = [MARKER.format(sha=SHA, verdict=verdict)]
    if verdict == "bug":
        lines.append(f"**Claude review of {short}: a bug blocks the merge.** {summary}".rstrip())
        for bug in bugs:
            where = f"`{bug.get('file', '?')}`" + (f" line {bug['line']}" if bug.get("line") else "")
            lines.append(f"- {where}: {bug.get('title', '')}")
        lines.append("")
        lines.append("Push a fix for a new review. If the finding is wrong, add the label `review-waived`"
                     " and start the failed run again.")
    elif verdict == "clean":
        lines.append(f"**Claude review of {short}: no bug found.** {summary}".rstrip())
    else:
        lines.append(f"**Claude review of {short}: {verdict}.** {summary}".rstrip())
    gh("api", f"repos/{REPO}/issues/{PR}/comments", "-f", "body=" + "\n".join(lines))


def failure_reason() -> str:
    """The API's or the action's own words for why no review came back."""
    path = os.environ.get("EXECUTION_FILE") or ""
    try:
        entries = json.load(open(path, encoding="utf-8"))
    except (OSError, ValueError):
        return "no result from the review step"
    for entry in entries if isinstance(entries, list) else []:
        if isinstance(entry, dict) and entry.get("type") == "result":
            if entry.get("is_error"):
                return str(entry.get("result") or "error")[:200]
            return "no structured verdict"
    return "no result from the review step"


def after() -> int:
    raw = os.environ.get("REVIEW_OUTPUT", "").strip()
    try:
        result = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        result = {}
    if not isinstance(result, dict) or result.get("verdict") not in {"bug", "clean"}:
        reason = failure_reason()
        print(f"The review returned no verdict ({reason}); see the run log. Not blocking the merge.")
        print(raw[:2000])
        post("unfinished", f"The review did not finish ({reason}), see the workflow run. The merge is not blocked.", [])
        return 0
    verdict = result["verdict"]
    bugs = [b for b in result.get("bugs") or [] if isinstance(b, dict)]
    if verdict == "bug" and not bugs:
        bugs = [{"title": "see the inline comments"}]
    post(verdict, str(result.get("summary") or "").strip(), bugs)
    print(f"Verdict: {verdict}")
    return 1 if verdict == "bug" else 0


if __name__ == "__main__":
    sys.exit({"before": before, "after": after}[sys.argv[1]]())
