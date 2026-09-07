#!/usr/bin/env python3
"""Per-crate line coverage, read back from a tarpaulin lcov.info.

A single workspace percentage says nothing actionable: balaur_core is 26k
lines and balaur_render is 18k of GPU paths a headless runner cannot enter,
so the total is mostly a weighted average of those two. Per crate is the
grain that matches how the engine is split and how a change lands.

Rows sort by uncovered lines, not by percentage, because that is the column
that says where the untested code actually is.

  python3 scripts/coverage_report.py target/coverage/lcov.info
  python3 scripts/coverage_report.py --markdown target/coverage/lcov.info
"""
from __future__ import annotations

import argparse
import collections
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def group_of(path: str) -> str:
    """The crate a source file belongs to, or its top directory otherwise."""
    p = pathlib.Path(path)
    if p.is_absolute():
        try:
            p = p.relative_to(ROOT)
        except ValueError:
            pass
    parts = p.parts
    if len(parts) >= 2 and parts[0] == "crates":
        return parts[1]
    return parts[0] if parts else "?"


def read_lcov(path: pathlib.Path) -> dict[str, tuple[int, int]]:
    """Covered and total lines per group, from DA records."""
    tally: dict[str, list[int]] = collections.defaultdict(lambda: [0, 0])
    group = "?"
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("SF:"):
            group = group_of(line[3:].strip())
        elif line.startswith("DA:"):
            _, _, rest = line.partition(":")
            _, _, hits = rest.partition(",")
            entry = tally[group]
            entry[1] += 1
            if hits.strip() not in ("0", ""):
                entry[0] += 1
    return {k: (v[0], v[1]) for k, v in tally.items()}


def pct(covered: int, total: int) -> float:
    return 100.0 * covered / total if total else 0.0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("lcov", nargs="?", default="target/coverage/lcov.info")
    ap.add_argument("--markdown", action="store_true", help="GitHub step summary form")
    args = ap.parse_args()

    path = pathlib.Path(args.lcov)
    if not path.is_file():
        print(f"no lcov at {path}; run scripts/coverage.sh first", file=sys.stderr)
        return 1

    tally = read_lcov(path)
    if not tally:
        print(f"{path} has no DA records", file=sys.stderr)
        return 1

    rows = sorted(tally.items(), key=lambda kv: kv[1][1] - kv[1][0], reverse=True)
    cov_all = sum(c for c, _ in tally.values())
    tot_all = sum(t for _, t in tally.values())

    if args.markdown:
        print("## Line coverage\n")
        print(f"**{pct(cov_all, tot_all):.2f}%** overall, {cov_all}/{tot_all} lines.\n")
        print("| Crate | Coverage | Covered | Uncovered |")
        print("| --- | ---: | ---: | ---: |")
        for name, (cov, tot) in rows:
            print(f"| {name} | {pct(cov, tot):.1f}% | {cov}/{tot} | {tot - cov} |")
    else:
        width = max(len(n) for n in tally)
        print(f"{'crate'.ljust(width)}  {'cover':>7}  {'covered':>12}  {'uncovered':>9}")
        for name, (cov, tot) in rows:
            print(
                f"{name.ljust(width)}  {pct(cov, tot):6.1f}%  "
                f"{f'{cov}/{tot}':>12}  {tot - cov:>9}"
            )
        print(f"\n{pct(cov_all, tot_all):.2f}% overall, {cov_all}/{tot_all} lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())
