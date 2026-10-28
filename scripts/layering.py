#!/usr/bin/env python3
"""`domain` stays pure: fails if its dependency tree (normal, build and dev, every target)
contains axum, sqlx or tokio, or any of their sub-crates (docs/spec/foundations.md#repository-layout).
"""

from __future__ import annotations

import re
import subprocess
import sys

CRATE = "domain"
FORBIDDEN = re.compile(r"^(axum|sqlx|tokio)(-[a-z0-9_-]+)?$")


def main() -> int:
    result = subprocess.run(
        ["cargo", "tree", "--locked", "-p", CRATE, "--edges", "normal,build,dev", "--target", "all",
         "--prefix", "none", "--format", "{p}"],
        capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        print(f"layering: cargo tree failed:\n{result.stderr}", file=sys.stderr)
        return 2
    names = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    bad = sorted(name for name in names if FORBIDDEN.match(name))
    if bad:
        print(f"layering: blocked: {CRATE} depends on {', '.join(bad)}", file=sys.stderr)
        print(f"  {CRATE} is pure logic; put I/O in db, actual, api or server.", file=sys.stderr)
        print(f"  See which dependency pulls it in: cargo tree -p {CRATE} -i {bad[0]}", file=sys.stderr)
        return 1
    print(f"layering: ok ({CRATE} has {len(names) - 1} dependencies, none of axum, sqlx, tokio)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
