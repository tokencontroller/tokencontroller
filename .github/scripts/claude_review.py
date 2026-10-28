"""The "Claude review" check: wait for the review's verdict, block only a bug.

The review itself runs as a scheduled task on claude.ai (a cloud session on
Jan's subscription, started by the pull request event), following the skill
reviewing-pull-requests in the private repo. It ends by posting one summary
comment on the pull request that carries a marker:

    <!-- claude-review sha=<head sha> verdict=bug|clean|skipped -->

This script runs in the check (.github/workflows/review.yml here, the
"Claude review" job in ci.yml in the public repo). It waits for that marker
on the head commit and fails only for the verdict `bug`. No wait for a
draft, for the label `review-waived`, or when the verdict is there already.
When no verdict arrives before the deadline, the check passes and says so:
a review that did not happen never blocks a merge.
"""

import json
import os
import subprocess
import sys
import time

REPO = os.environ["GITHUB_REPOSITORY"]
PR = os.environ["PR"]
SHA = os.environ["HEAD_SHA"]
MARKER = "<!-- claude-review sha={sha} verdict="
POLL = 60
DEADLINE = time.time() + 40 * 60
BLOCKS = {"bug"}


def gh(*args: str) -> str:
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout


def verdict() -> str | None:
    pages = json.loads(gh("api", "--paginate", "--slurp", f"repos/{REPO}/issues/{PR}/comments?per_page=100"))
    prefix = MARKER.format(sha=SHA)
    found = None
    for comment in (c for page in pages for c in page):
        body = comment.get("body") or ""
        start = body.find(prefix)
        if start >= 0:
            found = body[start + len(prefix):].split(" -->", 1)[0].strip()
    return found


def main() -> int:
    event = json.load(open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8"))
    pr = event["pull_request"]
    if pr.get("draft"):
        print("Draft pull request: no review until it is ready")
        return 0
    if any(label["name"] == "review-waived" for label in pr.get("labels", [])):
        print("Label review-waived: the review does not block this pull request")
        return 0
    while True:
        found = verdict()
        if found:
            print(f"Claude review of {SHA[:7]}: {found}")
            return 1 if found in BLOCKS else 0
        if time.time() > DEADLINE:
            print(f"No review arrived for {SHA[:7]} within the deadline; the merge is not blocked")
            return 0
        print("Waiting for the review")
        time.sleep(POLL)


if __name__ == "__main__":
    sys.exit(main())
