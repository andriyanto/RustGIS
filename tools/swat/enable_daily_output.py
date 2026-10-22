#!/usr/bin/env python3
"""Turn on daily, monthly, yearly and average annual output for every object in a SWAT+ print.prt.

The SWAT+ baseline runs (see docs/swat/design.md) need daily output so a port of the engine can be
compared day by day, not only on the average annual tables the reference data sets print.

Usage: enable_daily_output.py <scenario folder> [<scenario folder> ...]
"""

from __future__ import annotations

import sys
from pathlib import Path


def enable(prt: Path) -> int:
    """Rewrites the object table of `prt` with every flag set to `y`; returns the rows changed."""
    lines = prt.read_text(encoding="latin-1").splitlines()
    out: list[str] = []
    in_table = False
    changed = 0
    for line in lines:
        tokens = line.split()
        if tokens and tokens[0] == "objects":
            in_table = True
            out.append(line)
            continue
        if in_table and len(tokens) == 5 and all(t in ("y", "n") for t in tokens[1:]):
            out.append(f"{tokens[0]:<25}" + "".join(f"{'y':>14}" for _ in range(4)))
            changed += 1
            continue
        out.append(line)
    prt.write_text("\n".join(out) + "\n", encoding="latin-1")
    return changed


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    status = 0
    for folder in argv[1:]:
        prt = Path(folder) / "print.prt"
        if not prt.is_file():
            print(f"{prt}: not found", file=sys.stderr)
            status = 1
            continue
        changed = enable(prt)
        if changed == 0:
            print(f"{prt}: no object rows found", file=sys.stderr)
            status = 1
        else:
            print(f"{prt}: {changed} objects print daily, monthly, yearly and average annual")
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
