#!/usr/bin/env python3
"""Compare the numeric output tables of two SWAT+ runs, value by value.

Used twice in the SWAT+ port (docs/swat/design.md): to check a Fortran build against the reference
outputs committed in the SWAT+ repository, and later to check the Rust engine against the Fortran
baseline. The first line of each file (program name, version and run date) is skipped. Columns are
compared as numbers within `--abserr` or `--relerr`; text columns (names) must match exactly.

Usage:
  compare_outputs.py <expected folder> <actual folder> [--pattern '*_aa.txt'] [--abserr 1e-6]
                     [--relerr 1e-4] [--max-report 5] [--json summary.json]
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import math
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path


@dataclass
class FileResult:
    name: str
    status: str = "ok"  # ok | mismatch | missing | shape
    values: int = 0
    mismatches: int = 0
    max_abs: float = 0.0
    max_rel: float = 0.0
    examples: list[str] = field(default_factory=list)


def number(token: str) -> float | None:
    try:
        value = float(token)
    except ValueError:
        return None
    return value if math.isfinite(value) else None


def compare_file(
    expected: Path, actual: Path, abserr: float, relerr: float, max_report: int
) -> FileResult:
    result = FileResult(expected.name)
    if not actual.is_file():
        result.status = "missing"
        return result
    exp_lines = expected.read_text(encoding="latin-1").splitlines()[1:]
    act_lines = actual.read_text(encoding="latin-1").splitlines()[1:]
    if len(exp_lines) != len(act_lines):
        result.status = "shape"
        result.examples.append(
            f"{len(exp_lines)} lines expected, {len(act_lines)} found"
        )
        return result
    for row, (a_line, b_line) in enumerate(zip(exp_lines, act_lines), start=2):
        a_tok, b_tok = a_line.split(), b_line.split()
        if len(a_tok) != len(b_tok):
            result.status = "shape"
            result.examples.append(
                f"line {row}: {len(a_tok)} columns expected, {len(b_tok)} found"
            )
            return result
        for col, (a, b) in enumerate(zip(a_tok, b_tok), start=1):
            x, y = number(a), number(b)
            if x is None or y is None:
                if a != b:
                    result.mismatches += 1
                    if len(result.examples) < max_report:
                        result.examples.append(f"line {row} col {col}: {a!r} != {b!r}")
                continue
            result.values += 1
            diff = abs(x - y)
            rel = diff / max(abs(x), abs(y)) if diff > 0 else 0.0
            result.max_abs = max(result.max_abs, diff)
            result.max_rel = max(result.max_rel, rel)
            if diff > abserr and rel > relerr:
                result.mismatches += 1
                if len(result.examples) < max_report:
                    result.examples.append(
                        f"line {row} col {col}: {x} vs {y} (abs {diff:.3g}, rel {rel:.3g})"
                    )
    if result.mismatches:
        result.status = "mismatch"
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("expected", type=Path)
    parser.add_argument("actual", type=Path)
    parser.add_argument(
        "--pattern", default="*.txt", help="file name pattern (default: %(default)s)"
    )
    parser.add_argument(
        "--abserr",
        type=float,
        default=1e-6,
        help="absolute tolerance (default: %(default)s)",
    )
    parser.add_argument(
        "--relerr",
        type=float,
        default=1e-4,
        help="relative tolerance (default: %(default)s)",
    )
    parser.add_argument(
        "--max-report",
        type=int,
        default=5,
        help="examples shown per file (default: %(default)s)",
    )
    parser.add_argument("--json", type=Path, help="also write the summary as JSON")
    args = parser.parse_args()

    files = sorted(
        p
        for p in args.expected.iterdir()
        if p.is_file() and fnmatch.fnmatch(p.name, args.pattern)
    )
    if not files:
        print(f"no files match {args.pattern!r} in {args.expected}", file=sys.stderr)
        return 2
    results = [
        compare_file(f, args.actual / f.name, args.abserr, args.relerr, args.max_report)
        for f in files
    ]

    bad = 0
    for r in results:
        line = f"{r.status:<8} {r.name:<40} values={r.values:<9} max_abs={r.max_abs:.3g} max_rel={r.max_rel:.3g}"
        print(line)
        for example in r.examples:
            print(f"         {example}")
        bad += r.status != "ok"
    print(
        f"\n{len(results) - bad} of {len(results)} files within tolerance (abs {args.abserr}, rel {args.relerr})"
    )
    if args.json:
        args.json.write_text(
            json.dumps([asdict(r) for r in results], indent=1), encoding="utf-8"
        )
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
