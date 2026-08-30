#!/usr/bin/env python3
"""Run the benchmarks headless and print what each result means for a frame.

A raw nanosecond count says little. At 60 fps a frame is 16.667 ms, so what
matters is how much of it a workload eats — that is the number worth acting on.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRAME_NS = 16_666_667  # 60 fps


def run_benches(quick, only):
    args = ["cargo", "bench", "-p", "balaur_bench"]
    if only:
        args += ["--bench", only]
    args += ["--"]
    if quick:
        args += ["--warm-up-time", "0.4", "--measurement-time", "1.2", "--sample-size", "20"]
    subprocess.run(args, cwd=ROOT, check=True)


def collect():
    """Mean nanoseconds per iteration, per benchmark id."""
    out = {}
    for est in (ROOT / "target" / "criterion").rglob("new/estimates.json"):
        name = str(est.parent.parent.relative_to(ROOT / "target" / "criterion"))
        if name.endswith("/report"):
            continue
        try:
            out[name] = json.load(est.open())["mean"]["point_estimate"]
        except (KeyError, json.JSONDecodeError):
            continue
    return dict(sorted(out.items()))


def per_element(name):
    """Trailing /N in a benchmark id is the element count."""
    m = re.search(r"/(\d+)$", name)
    return int(m.group(1)) if m else None


def human(ns):
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.2f} µs"
    return f"{ns / 1_000_000:.2f} ms"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="fewer samples; noisier but ~1 min")
    ap.add_argument("--bench", help="only this bench target (scripting, engine)")
    ap.add_argument("--no-run", action="store_true", help="report the last run")
    args = ap.parse_args()

    if not args.no_run:
        run_benches(args.quick, args.bench)

    results = collect()
    if not results:
        print("no results; run without --no-run", file=sys.stderr)
        return 1

    width = max(len(n) for n in results)
    print(f"\n{'benchmark'.ljust(width)}  {'total':>10}  {'per item':>10}  {'per frame':>10}")
    print("-" * (width + 36))
    group = None
    for name, ns in results.items():
        head = name.split("/")[0]
        if head != group:
            group = head
            print()
        count = per_element(name)
        each = f"{human(ns / count):>10}" if count else " " * 10
        share = f"{100 * ns / FRAME_NS:>9.2f}%"
        print(f"{name.ljust(width)}  {human(ns):>10}  {each}  {share}")
    print(f"\nper frame is the share of one 60 fps frame ({human(FRAME_NS)}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
