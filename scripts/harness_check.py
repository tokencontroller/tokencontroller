#!/usr/bin/env python3
"""The harness checks of ADR 0003 and ADR 0013, run by CI on every pull request.

1. No test is skipped or switched off: no `#[ignore]`, no `#[should_panic]`
   and no always-false `#[cfg(...)]` anywhere.
2. No lint is switched off in our own code: no `allow`, `expect` or `warn`
   attributes, also not inside `cfg_attr`, so the strict lint block in
   Cargo.toml always applies.
3. Nothing switches the lint block off from outside: every Rust package is a
   workspace member that takes the lint block as it is
   (`[lints] workspace = true`), there is no Cargo config file, and there is
   no second clippy, rustfmt or toolchain file.
4. The service only runs SQL that is checked at compile time (ADR 0005): no
   `sqlx::query(...)` and friends, no query builder, no SQL text handed to an
   executor.
5. The number of tests never drops compared with the base branch.
6. Harness files that already exist on the base branch only change with the
   `harness-change` label set by the maintainer. Adding new ones (a new golden
   case, a new workflow) needs no label.

With --tests-ran it checks one more thing after the test run: every test in
the code ran, so a test file left out of the build cannot count as a test.

Usage: scripts/harness_check.py [BASE_REF]
       scripts/harness_check.py --tests-ran target/nextest/ci/junit.xml
Without BASE_REF checks 5 and 6 are skipped. Writes a plain summary to
$GITHUB_STEP_SUMMARY and the counts to $GITHUB_OUTPUT when those are set.
Exits 1 if any check fails.
"""

from __future__ import annotations

import bisect
import fnmatch
import os
import re
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path, PurePosixPath

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

# Config files that change what the compiler, Clippy or rustfmt check, by
# file name, and the one place each may live (check 3).
SINGLE_CONFIG_FILES = {
    "clippy.toml": "clippy.toml",
    ".clippy.toml": "clippy.toml",
    "rustfmt.toml": "rustfmt.toml",
    ".rustfmt.toml": "rustfmt.toml",
    "rust-toolchain.toml": "rust-toolchain.toml",
    "rust-toolchain": "rust-toolchain.toml",
}

# Check 4 covers the service's own code, before its test module.
SERVICE_SOURCE = "apps/server/src/"
SQLX_RUNTIME = r"query|query_as|query_scalar|query_with|query_as_with|query_scalar_with|raw_sql"
UNCHECKED_SQL = re.compile(
    rf"\bsqlx::(?:{SQLX_RUNTIME})\s*(?:::\s*<|\()"
    rf"|\buse\s+sqlx::(?:\{{[^}}]*\b(?:{SQLX_RUNTIME}|QueryBuilder|RawSql)\b|(?:{SQLX_RUNTIME})\s*;)"
    r"|\b(?:QueryBuilder|RawSql)\b"
    r"|\b(?:query|query_as|query_scalar|query_file|query_file_as|query_file_scalar)_unchecked\s*!"
    r"|\.(?:execute|execute_many|fetch|fetch_all|fetch_many|fetch_one|fetch_optional)"
    r"\s*\(\s*&?\s*(?:r#*\"|\"|format\s*!)"
)

ATTRIBUTE_START = re.compile(r"#\s*!?\s*\[")
TEST = re.compile(r"(?:test|tokio::test|sqlx::test)\b")
LINT_LEVEL = re.compile(r"(?:allow|expect|warn)\s*\(")
IGNORE = re.compile(r"ignore\b")
SHOULD_PANIC = re.compile(r"should_panic\b")
ALWAYS_OFF = re.compile(r"cfg\s*\(\s*(?:false|any\s*\(\s*\)|not\s*\(\s*all\s*\(\s*\)\s*\))\s*\)$")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def repo_files() -> list[str]:
    """Every file in the working tree that git does not ignore."""
    listing = git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
    return [p for p in listing.split("\0") if p and Path(p).exists()]


def rust_files_at(ref: str | None) -> dict[str, str]:
    """Every .rs file and its text, in the working tree or at a git ref."""
    if ref is None:
        return {p: Path(p).read_text(encoding="utf-8") for p in repo_files() if p.endswith(".rs")}
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


def blank_comments_and_strings(text: str) -> str:
    """Rust source with comments and the insides of string literals turned to
    spaces. Line breaks stay, so line numbers stay right."""
    out = list(text)
    i, n = 0, len(text)

    def blank(start: int, end: int) -> None:
        for k in range(start, end):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end == -1 else end
            blank(i, end)
            i = end
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == '"':
            hashes = 0
            while i - hashes - 1 >= 0 and text[i - hashes - 1] == "#":
                hashes += 1
            raw = i - hashes - 1 >= 0 and text[i - hashes - 1] == "r" and not (
                i - hashes - 2 >= 0 and (text[i - hashes - 2].isalnum() or text[i - hashes - 2] == "_")
                and text[i - hashes - 2] not in "bc"
            )
            j = i + 1
            if raw:
                closing = '"' + "#" * hashes
                end = text.find(closing, j)
                end = n if end == -1 else end
            else:
                end = j
                while end < n and text[end] != '"':
                    end += 2 if text[end] == "\\" else 1
                end = min(end, n)
            blank(j, end)
            i = end + 1
        elif c == "'":
            # A char literal ('a', '\n', '\u{1F600}'), or else a lifetime or label.
            if text.startswith("\\", i + 1):
                end = text.find("'", i + 3)
                end = n if end == -1 else end
                blank(i + 1, end)
                i = end + 1
            elif i + 2 < n and text[i + 2] == "'":
                blank(i + 1, i + 2)
                i += 3
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def split_top_level(text: str) -> list[str]:
    """Split at commas that are not inside brackets."""
    parts, depth, start = [], 0, 0
    for k, ch in enumerate(text):
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append(text[start:k].strip())
            start = k + 1
    parts.append(text[start:].strip())
    return [p for p in parts if p]


def effective(attribute: str) -> list[str]:
    """The attributes an attribute can turn into, looking inside cfg_attr."""
    m = re.fullmatch(r"cfg_attr\s*\((.*)\)", attribute)
    if not m:
        return [attribute]
    found = []
    for part in split_top_level(m.group(1))[1:]:
        found.extend(effective(part))
    return found


def attributes(text: str) -> list[tuple[int, str]]:
    """Every attribute in a Rust file as (line number, text inside #[...])."""
    code = blank_comments_and_strings(text)
    line_starts = [0] + [k + 1 for k, ch in enumerate(code) if ch == "\n"]
    found = []
    for m in ATTRIBUTE_START.finditer(code):
        depth, j = 1, m.end()
        while j < len(code) and depth:
            if code[j] == "[":
                depth += 1
            elif code[j] == "]":
                depth -= 1
            j += 1
        content = " ".join(code[m.end() : j - 1].split())
        found.append((bisect.bisect_right(line_starts, m.start()), content))
    return found


def count_tests(files: dict[str, str]) -> int:
    return sum(
        1 for text in files.values() for _, a in attributes(text) if TEST.match(a)
    )


def attribute_findings(files: dict[str, str], wanted) -> list[str]:
    found = []
    for path, text in sorted(files.items()):
        for line, content in attributes(text):
            if wanted(content):
                found.append(f"{path}:{line}: #[{content}]")
    return found


def lint_switch(content: str) -> bool:
    return any(LINT_LEVEL.match(a) for a in effective(content))


def skipped_test(content: str) -> bool:
    return any(IGNORE.match(a) or SHOULD_PANIC.match(a) for a in effective(content)) or bool(
        ALWAYS_OFF.match(content)
    )


def unchecked_sql(files: dict[str, str]) -> list[str]:
    found = []
    for path, text in sorted(files.items()):
        if not path.startswith(SERVICE_SOURCE):
            continue
        code = blank_comments_and_strings(text)
        line_starts = [0] + [k + 1 for k, ch in enumerate(code) if ch == "\n"]
        tests_from = min(
            (line for line, a in attributes(text) if re.fullmatch(r"cfg\s*\(\s*test\s*\)", a)),
            default=len(line_starts) + 1,
        )
        lines = text.splitlines()
        for m in UNCHECKED_SQL.finditer(code):
            number = bisect.bisect_right(line_starts, m.start())
            if number < tests_from:
                found.append(f"{path}:{number}: {lines[number - 1].strip()}")
    return found


def member_patterns(root: dict) -> tuple[list[str], list[str]]:
    workspace = root.get("workspace", {})
    clean = lambda items: [str(p).strip("/") for p in items]
    return clean(workspace.get("members", [])), clean(workspace.get("exclude", []))


def manifest_problems(files: list[str]) -> list[str]:
    """Rust packages that do not take the lint block as it is (check 3)."""
    problems = []
    root = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))
    members, excluded = member_patterns(root)
    for path in sorted(files):
        if PurePosixPath(path).name != "Cargo.toml":
            continue
        try:
            manifest = tomllib.loads(Path(path).read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            problems.append(f"{path}: not valid TOML ({error})")
            continue
        folder = str(PurePosixPath(path).parent)
        if path != "Cargo.toml":
            if "workspace" in manifest:
                problems.append(f"{path}: a second workspace; add the package to the root workspace instead")
                continue
            if "package" not in manifest:
                continue
            is_member = any(fnmatch.fnmatchcase(folder, p) for p in members) and not any(
                fnmatch.fnmatchcase(folder, p) for p in excluded
            )
            if not is_member:
                problems.append(f"{path}: not a member of the root workspace, so CI never builds, lints or tests it")
        if "package" in manifest and manifest.get("lints") != {"workspace": True}:
            problems.append(f"{path}: needs exactly `[lints] workspace = true` and no lints of its own")
    return problems


def config_problems(files: list[str]) -> list[str]:
    """Files that could switch lints, checks or overflow checks off (check 3)."""
    problems = []
    for path in sorted(files):
        p = PurePosixPath(path)
        if p.parent.name == ".cargo" and p.name in ("config", "config.toml"):
            problems.append(
                f"{path}: Cargo config files are not allowed; they can change lints, "
                "compiler flags and profiles outside the lint block"
            )
        elif p.name in SINGLE_CONFIG_FILES and path != SINGLE_CONFIG_FILES[p.name]:
            problems.append(f"{path}: only the root {SINGLE_CONFIG_FILES[p.name]} may exist")
    return problems


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


def changed_harness_files(merge_base: str) -> list[str]:
    """Harness files that existed at the merge base and were changed or deleted."""
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


def report(title: str, summary: list[str], failures: list[str], outputs: dict[str, str]) -> int:
    for line in summary:
        print(f"ok: {line}")
    for failure in failures:
        print(f"FAILED: {failure}", file=sys.stderr)
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as out:
            out.write(f"### {title}\n\n")
            for line in summary:
                out.write(f"- {line}\n")
            for failure in failures:
                out.write(f"- **Failed:** {failure.splitlines()[0]}\n")
            out.write("\n")
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as out:
            for key, value in outputs.items():
                out.write(f"{key}={value}\n")
    return 1 if failures else 0


def tests_ran(junit: str) -> int:
    """Check that every test in the code ran: nextest ran at least as many
    tests as there are test functions in the source."""
    in_code = count_tests(rust_files_at(None))
    run = ET.parse(junit).getroot()
    ran = int(run.get("tests", "0")) - int(run.get("skipped", "0"))
    failures, summary = [], []
    if ran < in_code:
        failures.append(
            f"The code has {in_code} tests but only {ran} ran. A test file is not part of "
            "the build (a missing `mod`), or a test is switched off by a cfg."
        )
    else:
        summary.append(f"Every test in the code ran: {in_code} in the code, {ran} ran.")
    return report("Tests", summary, failures, {"tests_in_code": str(in_code), "tests_ran": str(ran)})


def main() -> int:
    if len(sys.argv) == 3 and sys.argv[1] == "--tests-ran":
        return tests_ran(sys.argv[2])
    base = sys.argv[1] if len(sys.argv) > 1 else None
    files = repo_files()
    rust = rust_files_at(None)
    failures: list[str] = []
    summary: list[str] = []

    skipped = attribute_findings(rust, skipped_test)
    if skipped:
        failures.append(
            "Tests may not be skipped, expected to crash or switched off:\n  " + "\n  ".join(skipped)
        )
    else:
        summary.append("No test is skipped, expected to crash or switched off.")

    lint_switches = attribute_findings(rust, lint_switch)
    if lint_switches:
        failures.append(
            "Lints may not be switched off in code (ADR 0013); fix the code instead:\n  "
            + "\n  ".join(lint_switches)
        )
    else:
        summary.append("No lint is switched off in the code.")

    outside = manifest_problems(files) + config_problems(files)
    if outside:
        failures.append(
            "Lints must apply to every Rust package, set only by the lint block (ADR 0013):\n  "
            + "\n  ".join(outside)
        )
    else:
        summary.append("Every Rust package takes the lint block, and no file switches it off.")

    sql = unchecked_sql(rust)
    if sql:
        failures.append(
            "The service may only run SQL checked at compile time (ADR 0005); "
            "use sqlx::query! and its siblings:\n  " + "\n  ".join(sql)
        )
    else:
        summary.append("The service runs only SQL checked at compile time.")

    tests_now = count_tests(rust)
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

        changed = changed_harness_files(merge_base)
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

    return report(
        "Harness", summary, failures, {"tests": str(tests_now), "golden": str(golden_now)}
    )


if __name__ == "__main__":
    sys.exit(main())
