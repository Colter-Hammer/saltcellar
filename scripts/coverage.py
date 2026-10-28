#!/usr/bin/env python3
"""Line coverage of each crate in coverage-floors.txt is at or above its floor, and no floor is
lower than on main (scripts/base.py). Runs the crate's tests under cargo-llvm-cov.

coverage-floors.txt: one `<crate> <percent>` per line; `#` starts a comment.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from base import base_commit, show

FILE = "coverage-floors.txt"
CHECK = "coverage"


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
    result = subprocess.run(
        ["cargo", "llvm-cov", "nextest", "--locked", "-p", crate, "--json", "--summary-only"],
        stdout=subprocess.PIPE, text=True, check=False,
    )
    if result.returncode != 0:
        print(f"{CHECK}: cargo llvm-cov failed for {crate}", file=sys.stderr)
        sys.exit(2)
    return json.loads(result.stdout)["data"][0]["totals"]["lines"]["percent"]


def main() -> int:
    floors = parse(Path(FILE).read_text())
    problems = []

    base = base_commit(CHECK)
    shipped = show(base, FILE) if base else None
    for crate, old in (parse(shipped.decode()) if shipped else {}).items():
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
