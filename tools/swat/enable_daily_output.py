#!/usr/bin/env python3
"""Turn on daily, monthly, yearly and average annual output in a SWAT+ print.prt.

The SWAT+ baseline runs (see docs/swat/design.md) need daily output so a port of the engine can be
compared day by day, not only on the average annual tables the reference data sets print. With
`--only`, just the named objects are changed (the others keep their settings), which keeps the
baseline small; without it, every object prints at every interval.

Usage: enable_daily_output.py [--only basin_wb,hru_wb,...] <scenario folder> [<scenario folder> ...]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


def enable(prt: Path, only: set[str] | None) -> tuple[int, set[str]]:
    """Sets every flag of the chosen object rows to `y`; returns the rows changed and the names seen."""
    lines = prt.read_text(encoding="latin-1").splitlines()
    out: list[str] = []
    in_table = False
    changed = 0
    seen: set[str] = set()
    for line in lines:
        tokens = line.split()
        if tokens and tokens[0] == "objects":
            in_table = True
            out.append(line)
            continue
        if in_table and len(tokens) == 5 and all(t in ("y", "n") for t in tokens[1:]):
            seen.add(tokens[0])
            if only is None or tokens[0] in only:
                out.append(f"{tokens[0]:<25}" + "".join(f"{'y':>14}" for _ in range(4)))
                changed += 1
                continue
        out.append(line)
    prt.write_text("\n".join(out) + "\n", encoding="latin-1")
    return changed, seen


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--only", help="comma-separated objects to change (default: all)"
    )
    parser.add_argument("folders", nargs="+", type=Path)
    args = parser.parse_args()
    only = (
        {name.strip() for name in args.only.split(",") if name.strip()}
        if args.only
        else None
    )

    status = 0
    for folder in args.folders:
        prt = folder / "print.prt"
        if not prt.is_file():
            print(f"{prt}: not found", file=sys.stderr)
            status = 1
            continue
        changed, seen = enable(prt, only)
        if only:
            unknown = sorted(only - seen)
            if unknown:
                # Scenarios list different objects; a name missing from one is only noted.
                print(f"{prt}: not in its object table, skipped: {', '.join(unknown)}")
        if changed == 0:
            print(f"{prt}: no object rows changed", file=sys.stderr)
            status = 1
        else:
            print(
                f"{prt}: {changed} objects print daily, monthly, yearly and average annual"
            )
    return status


if __name__ == "__main__":
    sys.exit(main())
