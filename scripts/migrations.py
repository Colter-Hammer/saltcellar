#!/usr/bin/env python3
"""Migrations are append-only: a migration that exists on `main` is never edited or deleted.

Compares migrations/ with the base commit on main (scripts/base.py). Also checks that every
migration is named NNNN_snake_case.sql and that new ones sort after every migration on main,
so an install that already ran main's migrations can run the new ones in order.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

from base import base_commit, git, show

DIR = "migrations"
NAME = re.compile(r"^(\d{4})_[a-z0-9_]+\.sql$")
CHECK = "migrations"


def on_base(commit: str) -> list[str]:
    out = git("ls-tree", "--name-only", f"{commit}:{DIR}")
    return out.stdout.decode().split() if out.returncode == 0 else []


def main() -> int:
    here = sorted(p.name for p in Path(DIR).iterdir()) if Path(DIR).is_dir() else []
    problems = [
        f"{DIR}/{name}: name it NNNN_snake_case.sql, e.g. 0002_add_accounts.sql"
        for name in here
        if not NAME.match(name)
    ]

    base = base_commit(CHECK)
    shipped = on_base(base) if base else []
    for name in shipped:
        path = Path(DIR, name)
        if not path.is_file():
            problems.append(f"{path}: shipped on main and deleted; restore it")
        elif path.read_bytes() != show(base, f"{DIR}/{name}"):
            problems.append(f"{path}: shipped on main and edited; restore it and add a new migration instead")

    versions = [int(m.group(1)) for name in shipped if (m := NAME.match(name))]
    newest = max(versions, default=0)
    for name in here:
        m = NAME.match(name)
        if name not in shipped and m and int(m.group(1)) <= newest:
            problems.append(f"{DIR}/{name}: new migrations are numbered after {newest:04d}, main's newest")

    if problems:
        print(f"{CHECK}: blocked:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print(f"{CHECK}: ok ({len(shipped)} shipped, {len(here) - len(shipped)} new)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
