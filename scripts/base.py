"""The commit on `main` a branch is compared with, for the checks that only allow some changes.

The fork point with `origin/main`: on a pull request that is up to date (CI requires it), that's
main's tip. On `main` itself it's the commit before. Set CHECK_BASE to compare with another ref.

Standard library only.
"""

from __future__ import annotations

import os
import subprocess
import sys


def git(*args: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(["git", *args], capture_output=True, check=False)


def die(name: str, message: str) -> None:
    print(f"{name}: {message}", file=sys.stderr)
    sys.exit(2)


def base_commit(name: str) -> str | None:
    """The base commit, or None when main has nothing yet to compare with (its first commit)."""
    ref = os.environ.get("CHECK_BASE") or "origin/main"
    if git("rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}").returncode != 0:
        die(name, f"can't find {ref} to compare with; run `git fetch origin main` (or set CHECK_BASE)")
    head = git("rev-parse", "HEAD").stdout.decode().strip()
    fork = git("merge-base", "HEAD", ref)
    if fork.returncode != 0:
        die(name, f"HEAD shares no history with {ref}")
    base = fork.stdout.decode().strip()
    if base == head:
        parent = git("rev-parse", "--verify", "--quiet", "HEAD^")
        return parent.stdout.decode().strip() if parent.returncode == 0 else None
    return base


def show(commit: str, path: str) -> bytes | None:
    """A file's bytes at `commit`, or None if it isn't there."""
    result = git("show", f"{commit}:{path}")
    return result.stdout if result.returncode == 0 else None
