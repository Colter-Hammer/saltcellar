#!/usr/bin/env python3
"""Line coverage of each entry in coverage-floors.txt is at or above its floor, and no floor is
lower than on main (scripts/base.py). A crate's tests run under cargo-llvm-cov; `web` is the
frontend's logic, measured by Vitest (web/vite.config.ts says which files).

    coverage.py [rust|web]   only the crates, or only the frontend; both by default

coverage-floors.txt: one `<crate or web> <percent>` per line; `#` starts a comment.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from base import base_commit, show

FILE = "coverage-floors.txt"
CHECK = "coverage"
WEB = "web"


def parse(text: str) -> dict[str, float]:
    floors = {}
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        try:
            crate, floor = parts[0], float(parts[1])
            if len(parts) != 2 or not 0 <= floor <= 100:
                raise ValueError
        except (IndexError, ValueError):
            print(f"{CHECK}: {FILE}:{lineno}: expected `<crate> <percent>`, got {raw!r}", file=sys.stderr)
            sys.exit(2)
        floors[crate] = floor
    return floors


def measure(crate: str) -> float:
    if crate == WEB:
        return measure_web()
    result = subprocess.run(
        ["cargo", "llvm-cov", "nextest", "--locked", "-p", crate, "--json", "--summary-only"],
        stdout=subprocess.PIPE, text=True, check=False,
    )
    if result.returncode != 0:
        print(f"{CHECK}: cargo llvm-cov failed for {crate}", file=sys.stderr)
        sys.exit(2)
    return json.loads(result.stdout)["data"][0]["totals"]["lines"]["percent"]


def measure_web() -> float:
    result = subprocess.run(
        ["npm", "--prefix", WEB, "run", "coverage", "--", "--coverage.reporter=json-summary"],
        stdout=subprocess.DEVNULL, check=False,
    )
    if result.returncode != 0:
        print(f"{CHECK}: vitest coverage failed for {WEB}", file=sys.stderr)
        sys.exit(2)
    summary = json.loads(Path(WEB, "coverage", "coverage-summary.json").read_text())
    return summary["total"]["lines"]["pct"]


def selected(name: str, only: str | None) -> bool:
    return only is None or (name == WEB) == (only == WEB)


def main() -> int:
    only = sys.argv[1] if len(sys.argv) > 1 else None
    if only not in (None, "rust", WEB) or len(sys.argv) > 2:
        print(f"usage: coverage.py [rust|{WEB}]", file=sys.stderr)
        return 2
    floors = {name: floor for name, floor in parse(Path(FILE).read_text()).items() if selected(name, only)}
    problems = []

    base = base_commit(CHECK)
    shipped = show(base, FILE) if base else None
    for crate, old in (parse(shipped.decode()) if shipped else {}).items():
        if not selected(crate, only):
            continue
        if crate not in floors:
            problems.append(f"{crate}: the floor ({old:g}%) was removed; coverage floors only go up")
        elif floors[crate] < old:
            problems.append(f"{crate}: the floor went down from {old:g}% to {floors[crate]:g}%; floors only go up")

    for crate, floor in floors.items():
        percent = measure(crate)
        if percent < floor:
            problems.append(f"{crate}: {percent:.2f}% of lines covered, under the {floor:g}% floor; add tests")
        else:
            print(f"{CHECK}: {crate}: {percent:.2f}% of lines covered (floor {floor:g}%)")
            if percent >= floor + 1:
                print(f"  raise the floor in {FILE} to lock it in: {crate} {int(percent * 10) / 10:g}")

    if problems:
        print(f"{CHECK}: blocked:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print(f"{CHECK}: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
