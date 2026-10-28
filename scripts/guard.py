#!/usr/bin/env python3
"""Repo guards that keep personal data out of git (docs/adr/0008-nothing-personal-in-the-repo.md).

Two checks, each over one scope:

    guard.py leak  staged|push|history   denylisted terms in file contents, paths and commit messages
    guard.py files staged|push|history   denied file types outside fixtures/, and files over 1 MiB
    guard.py leak  msg <file>            denylisted terms in a commit message file (commit-msg hook)

Scopes: `staged` is the index (pre-commit); `push` is the commits being pushed (pre-push, via
scripts/pre-push, which puts git's list of pushed commits in GUARD_PUSH_REVS; run by hand without
it, every commit on a local branch or tag that no remote has yet); `history` is every commit
reachable from any ref (CI, `just check`).

The denylist is personal, so it never lives in the repo. It is read from the path in
BUDGET_LEAK_DENYLIST, default ~/.config/budget-leak-denylist.txt. One term per line, matched
case-insensitively as a whole word (no letter or digit on either side; a space matches any run of
whitespace). A line starting with `re:` is a regex instead. Lines starting with `#` are comments.
There are no excluded paths. With LEAK_GUARD_REDACT=1 findings name the denylist line instead of
the term, for logs that leave the PC.

Standard library only, so it runs before any project dependency is installed.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_DENYLIST = "~/.config/budget-leak-denylist.txt"
MAX_BYTES = 1024 * 1024

# Data-dump types: only allowed inside a `fixtures/` directory (generated fake data).
FIXTURE_ONLY = re.compile(r"\.(csv|ofx|qfx|qbo|json|zip|sqlite[^/]*)$", re.IGNORECASE)
# Tool config files that have to be JSON; keep in sync with .gitignore.
TOOL_JSON = re.compile(
    r"(^|/)(package\.json|package-lock\.json|tsconfig[^/]*\.json|renovate\.json)$"
    r"|^\.claude/settings\.json$"
    r"|^web/components\.json$"
    r"|^\.sqlx/query-[0-9a-f]{64}\.json$"
)
# Never allowed anywhere, fixtures included.
ALWAYS_DENIED = [
    (re.compile(r"(^|/)\.env[^/]*$"), "env files are never committed"),
    (re.compile(r"\.age$", re.IGNORECASE), "encrypted backups are never committed"),
    (re.compile(r"\.(bak|backup)$", re.IGNORECASE), "backups are never committed"),
    (re.compile(r"(^|/)(data|backups?)/"), "data/ and backup folders are never committed"),
]

PUSH_REVS = ["--branches", "--tags", "--not", "--remotes"]
HISTORY_REVS = ["--all"]


class Term:
    def __init__(self, text: str, lineno: int) -> None:
        self.lineno = lineno
        self.is_regex = text.startswith("re:")
        if self.is_regex:
            self.text = text[3:]
            self.regex = re.compile(self.text, re.IGNORECASE)
        else:
            self.text = text
            body = r"\s+".join(re.escape(word) for word in text.split())
            self.regex = re.compile(rf"(?<![a-z0-9]){body}(?![a-z0-9])", re.IGNORECASE)

    def label(self, matched: str) -> str:
        """The term for a plain line; the matched text for a regex line, whose pattern says little."""
        if os.environ.get("LEAK_GUARD_REDACT") == "1":
            return f"denylist line {self.lineno}"
        if self.is_regex:
            return f'"{matched}" (denylist line {self.lineno})'
        return f'"{self.text}"'


def die(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(2)


def git(*args: str, stdin: bytes | None = None) -> bytes:
    result = subprocess.run(["git", *args], input=stdin, capture_output=True, check=False)
    if result.returncode != 0:
        die(f"guard: git {' '.join(args)} failed:\n{result.stderr.decode(errors='replace')}")
    return result.stdout


def load_denylist() -> list[Term]:
    path = Path(os.environ.get("BUDGET_LEAK_DENYLIST") or DEFAULT_DENYLIST).expanduser()
    where = (
        f"Put the denylist at {path} (one term per line, see CONTRIBUTING.md),\n"
        "or point BUDGET_LEAK_DENYLIST at it. The leak guard never passes without one."
    )
    if not path.is_file():
        die(f"leak-guard: no denylist found at {path}.\n{where}")
    terms = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            terms.append(Term(line, lineno))
        except re.error as error:
            die(f"leak-guard: {path}:{lineno}: bad regex: {error}")
    if not terms:
        die(f"leak-guard: the denylist at {path} has no terms.\n{where}")
    return terms


def parse_raw(output: bytes) -> list[tuple[str, str]]:
    """(path, blob sha) for each added or modified file in `git diff --raw -z --no-renames` output."""
    tokens = output.split(b"\0")
    changes = []
    i = 0
    while i < len(tokens):
        meta = tokens[i]
        if meta.startswith(b":"):
            _, new_mode, _, new_sha, _ = meta[1:].decode().split(" ")
            path = tokens[i + 1].decode(errors="replace")
            if new_mode != "160000":  # submodule, not a blob
                changes.append((path, new_sha))
            i += 2
        else:
            i += 1
    return changes


def changes_in(scope: str) -> dict[str, list[str]]:
    """Blob sha -> the places it appears (path, plus the commit for push/history)."""
    blobs: dict[str, list[str]] = {}
    if scope == "staged":
        raw = git("diff", "--cached", "--raw", "-z", "--no-abbrev", "--no-renames", "--diff-filter=ACMRT")
        for path, sha in parse_raw(raw):
            blobs.setdefault(sha, []).append(path)
        return blobs
    for commit in commits_in(scope):
        raw = git(
            "diff-tree", "-r", "-z", "-m", "--no-abbrev", "--root", "--no-commit-id", "--no-renames",
            "--diff-filter=ACMRT", commit,
        )
        for path, sha in parse_raw(raw):
            blobs.setdefault(sha, []).append(f"{path} (commit {commit[:10]})")
    return blobs


def revs_for(scope: str) -> list[str]:
    if scope != "push":
        return HISTORY_REVS
    pushed = os.environ.get("GUARD_PUSH_REVS", "").split()
    return [*pushed, "--not", "--remotes"] if pushed else PUSH_REVS


def commits_in(scope: str) -> list[str]:
    return git("rev-list", *revs_for(scope)).decode().split()


def messages_in(scope: str) -> list[tuple[str, str]]:
    if scope == "staged":
        return []
    out = git("log", "-z", "--format=%H%n%B", *revs_for(scope)).decode(errors="replace")
    messages = []
    for entry in filter(None, out.split("\0")):
        sha, _, body = entry.partition("\n")
        messages.append((f"commit {sha[:10]} message", body))
    return messages


def read_blobs(shas: list[str]) -> dict[str, bytes]:
    out = git("cat-file", "--batch", stdin="".join(f"{sha}\n" for sha in shas).encode())
    blobs = {}
    pos = 0
    for sha in shas:
        header_end = out.index(b"\n", pos)
        size = int(out[pos:header_end].split(b" ")[2])
        blobs[sha] = out[header_end + 1 : header_end + 1 + size]
        pos = header_end + 1 + size + 1
    return blobs


def blob_sizes(shas: list[str]) -> dict[str, int]:
    out = git("cat-file", "--batch-check", stdin="".join(f"{sha}\n" for sha in shas).encode())
    return {line.split()[0]: int(line.split()[2]) for line in out.decode().splitlines()}


def find_terms(terms: list[Term], text: str) -> list[tuple[str, int]]:
    """(label, line number) for every denylist match in the text."""
    hits = []
    for term in terms:
        for match in term.regex.finditer(text):
            hits.append((term.label(match.group()), text.count("\n", 0, match.start()) + 1))
    return hits


def where(places: list[str]) -> str:
    more = f" (+{len(places) - 1} more)" if len(places) > 1 else ""
    return places[0] + more


def check_leak(scope: str, msg_file: str | None) -> list[str]:
    terms = load_denylist()
    findings = []
    if scope == "msg":
        text = Path(msg_file or "").read_text(encoding="utf-8", errors="replace")
        text = "\n".join(line for line in text.splitlines() if not line.startswith("#"))
        for label, lineno in find_terms(terms, text):
            findings.append(f"{label} in the commit message, line {lineno}")
        return findings
    blobs = changes_in(scope)
    seen_paths = set()
    for sha, places in blobs.items():
        for place in places:
            path = place.split(" (commit ")[0]
            if path not in seen_paths:
                seen_paths.add(path)
                for label, _ in find_terms(terms, path):
                    findings.append(f"{label} in the file name {path}")
    contents = read_blobs(list(blobs))
    for sha, data in contents.items():
        for label, lineno in find_terms(terms, data.decode("utf-8", errors="replace")):
            findings.append(f"{label} in {where(blobs[sha])}, line {lineno}")
    for place, body in messages_in(scope):
        for label, lineno in find_terms(terms, body):
            findings.append(f"{label} in {place}, line {lineno}")
    return findings


def file_problem(path: str) -> str | None:
    for pattern, reason in ALWAYS_DENIED:
        if pattern.search(path):
            return reason
    in_fixtures = "fixtures" in path.split("/")[:-1]
    if FIXTURE_ONLY.search(path) and not in_fixtures and not TOOL_JSON.search(path):
        kind = path.rsplit(".", 1)[-1].lower()
        if kind == "json":
            return "JSON is only allowed under fixtures/ or as a tool config listed in scripts/guard.py (TOOL_JSON)"
        return f"{kind} files are only allowed under fixtures/ (generated fake data)"
    return None


def check_files(scope: str) -> list[str]:
    blobs = changes_in(scope)
    sizes = blob_sizes(list(blobs))
    findings = []
    seen = set()
    for sha, places in blobs.items():
        for place in places:
            path = place.split(" (commit ")[0]
            problem = file_problem(path)
            if problem and path not in seen:
                seen.add(path)
                findings.append(f"{place}: {problem}")
        if sizes[sha] > MAX_BYTES:
            findings.append(f"{where(places)}: {sizes[sha] / MAX_BYTES:.1f} MiB is over the 1 MiB limit")
    return findings


def main(argv: list[str]) -> int:
    usage = "usage: guard.py leak|files staged|push|history   or   guard.py leak msg <file>"
    if len(argv) < 2 or argv[0] not in ("leak", "files"):
        die(usage)
    check, scope = argv[0], argv[1]
    if scope == "msg" and check == "leak" and len(argv) == 3:
        findings = check_leak(scope, argv[2])
    elif scope in ("staged", "push", "history") and len(argv) == 2:
        findings = check_leak(scope, None) if check == "leak" else check_files(scope)
    else:
        die(usage)
    name = "leak-guard" if check == "leak" else "file-rule"
    if findings:
        print(f"{name}: blocked ({scope}):", file=sys.stderr)
        for finding in findings:
            print(f"  {finding}", file=sys.stderr)
        if check == "leak":
            print("Remove the term (or replace it with fake data) and try again.", file=sys.stderr)
        return 1
    print(f"{name}: ok ({scope})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
