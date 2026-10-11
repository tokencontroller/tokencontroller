#!/usr/bin/env python3
"""The harness checks of ADR 0003 and ADR 0013, run by CI on every pull request.

1. No test is skipped: `#[ignore]` is not allowed anywhere.
2. No lint is switched off in our own code: no `allow`, `expect` or `warn`
   attributes, so the strict lint block in Cargo.toml always applies.
3. The number of tests never drops compared with the base branch.
4. Harness files that already exist on the base branch only change with the
   `harness-change` label set by the maintainer. Adding new ones (a new golden
   case, a new workflow) needs no label.

Usage: scripts/harness_check.py [BASE_REF]
Without BASE_REF only checks 1 and 2 run. Writes a plain summary to
$GITHUB_STEP_SUMMARY when that is set. Exits 1 if any check fails.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

MAINTAINER = "jancbeck"
LABEL = "harness-change"

# Files and folders owned by the maintainer (ADR 0003).
HARNESS_PATHS = (
    "clippy.toml",
    "deny.toml",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".config/nextest.toml",
    ".cargo/mutants.toml",
    ".github/workflows/",
    "scripts/harness_check.py",
    "scripts/smoke-test",
    "tests/golden/",
)
# Parts of Cargo.toml owned by the maintainer: the lint block and the profiles.
CARGO_HARNESS_KEYS = (("workspace", "lints"), ("profile",))

TEST_ATTRIBUTE = re.compile(r"#\[\s*(test|tokio::test|sqlx::test)\b")
IGNORE_ATTRIBUTE = re.compile(r"#\[\s*ignore\b")
LINT_ATTRIBUTE = re.compile(r"#!?\[\s*(?:cfg_attr\s*\(.*?,\s*)?(allow|expect|warn)\s*\(")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def rust_files_at(ref: str | None) -> dict[str, str]:
    """Every .rs file and its text, in the working tree or at a git ref."""
    if ref is None:
        paths = git("ls-files", "--cached", "--others", "--exclude-standard", "*.rs").split()
        return {p: Path(p).read_text(encoding="utf-8") for p in paths if Path(p).exists()}
    paths = git("ls-tree", "-r", "--name-only", ref).split()
    return {p: git("show", f"{ref}:{p}") for p in paths if p.endswith(".rs")}


def golden_cases_at(ref: str | None) -> int:
    if ref is None:
        root = Path("tests/golden")
        return sum(1 for p in root.iterdir() if p.is_dir()) if root.exists() else 0
    try:
        listing = git("ls-tree", "-d", "--name-only", f"{ref}:tests/golden")
    except subprocess.CalledProcessError:
        return 0
    return len(listing.split())


def count_tests(files: dict[str, str]) -> int:
    return sum(len(TEST_ATTRIBUTE.findall(text)) for text in files.values())


def findings(files: dict[str, str], pattern: re.Pattern[str]) -> list[str]:
    found = []
    for path, text in sorted(files.items()):
        for number, line in enumerate(text.splitlines(), start=1):
            code = line.split("//", 1)[0]
            if pattern.search(code):
                found.append(f"{path}:{number}: {line.strip()}")
    return found


def cargo_harness_part(text: str | None) -> dict:
    if text is None:
        return {}
    data = tomllib.loads(text)
    part = {}
    for keys in CARGO_HARNESS_KEYS:
        node = data
        for key in keys:
            node = node.get(key, {}) if isinstance(node, dict) else {}
        part[".".join(keys)] = node
    return part


def changed_harness_files(base: str) -> list[str]:
    """Harness files that existed at the merge base and were changed or deleted."""
    merge_base = git("merge-base", base, "HEAD").strip()
    changed = []
    for line in git("diff", "--name-status", "--no-renames", merge_base).splitlines():
        status, path = line.split("\t", 1)
        if status == "A":
            continue
        if any(path == h or (h.endswith("/") and path.startswith(h)) for h in HARNESS_PATHS):
            changed.append(path)
    try:
        before = git("show", f"{merge_base}:Cargo.toml")
    except subprocess.CalledProcessError:
        before = None
    if before is not None:
        after = Path("Cargo.toml").read_text(encoding="utf-8") if Path("Cargo.toml").exists() else ""
        if cargo_harness_part(before) != cargo_harness_part(after):
            changed.append("Cargo.toml (lint block or profiles)")
    return changed


def main() -> int:
    base = sys.argv[1] if len(sys.argv) > 1 else None
    files = rust_files_at(None)
    failures: list[str] = []
    summary: list[str] = []

    ignored = findings(files, IGNORE_ATTRIBUTE)
    if ignored:
        failures.append("Skipped tests are not allowed:\n  " + "\n  ".join(ignored))
    else:
        summary.append("No test is skipped.")

    lint_switches = findings(files, LINT_ATTRIBUTE)
    if lint_switches:
        failures.append(
            "Lints may not be switched off in code (ADR 0013); fix the code instead:\n  "
            + "\n  ".join(lint_switches)
        )
    else:
        summary.append("No lint is switched off in the code.")

    tests_now = count_tests(files)
    golden_now = golden_cases_at(None)
    summary.append(f"{tests_now} tests and {golden_now} golden cases.")

    if base is not None:
        merge_base = git("merge-base", base, "HEAD").strip()
        tests_before = count_tests(rust_files_at(merge_base))
        golden_before = golden_cases_at(merge_base)
        if tests_now < tests_before:
            failures.append(
                f"The number of tests dropped from {tests_before} to {tests_now}."
            )
        if golden_now < golden_before:
            failures.append(
                f"The number of golden cases dropped from {golden_before} to {golden_now}."
            )

        changed = changed_harness_files(base)
        approved_by = os.environ.get("HARNESS_LABEL_BY", "")
        if changed and approved_by != MAINTAINER:
            failures.append(
                f"Harness files changed; this needs the {LABEL!r} label from {MAINTAINER}:\n  "
                + "\n  ".join(changed)
            )
        elif changed:
            summary.append(f"Harness change approved by {MAINTAINER}: {', '.join(changed)}.")
        else:
            summary.append("No harness file was loosened.")

    for line in summary:
        print(f"ok: {line}")
    for failure in failures:
        print(f"FAILED: {failure}", file=sys.stderr)

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as out:
            out.write("### Harness\n\n")
            for line in summary:
                out.write(f"- {line}\n")
            for failure in failures:
                out.write(f"- **Failed:** {failure.splitlines()[0]}\n")
            out.write("\n")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
