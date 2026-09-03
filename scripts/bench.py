#!/usr/bin/env python3
"""Run the benchmarks headless and print what each result means for a frame.

A raw nanosecond count says little. At 60 fps a frame is 16.667 ms, so what
matters is how much of it a workload eats — that is the number worth acting on.

crates/balaur_bench/budgets.toml holds one ceiling per benchmark, and is the
only place a performance number lives: `--record` writes it from a real run,
`--check` reports against it, and tests/budgets.rs gates on it in CI. The
ceilings are not measurements — they carry enough headroom to survive a shared
runner, so a regression in kind fails and a slow morning does not.
"""
import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUDGETS = ROOT / "crates" / "balaur_bench" / "budgets.toml"
# What a recorded number is multiplied by to become a ceiling. Ten, because a
# gate that cries wolf on a shared runner gets ignored, which is worse than no
# gate — the doctrine tests/budgets.rs was already written to.
HEADROOM = 10.0
FRAME_NS = 16_666_667  # 60 fps


def bench_targets():
    """The crate's `[[bench]]` targets, by name.

    Named one by one because `--benches` means every target with `bench = true`
    — the lib included, which runs under libtest and rejects criterion's flags.
    """
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    package = next(
        p for p in json.loads(out.stdout)["packages"] if p["name"] == "balaur_bench"
    )
    return [t["name"] for t in package["targets"] if "bench" in t["kind"]]


def run_benches(quick, only):
    args = ["cargo", "bench", "-p", "balaur_bench"]
    for name in [only] if only else bench_targets():
        args += ["--bench", name]
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


def budgets():
    """Ceilings, by benchmark id. Absent file means nothing is gated."""
    if not BUDGETS.exists():
        return {}
    with BUDGETS.open("rb") as f:
        return tomllib.load(f).get("ceiling_ns", {})


def record(results):
    """Rewrite the budget file from this run, ceilings at HEADROOM times."""
    lines = [
        "# The one place a performance number lives: written by `bench.py --record`\n",
        f"# from a real run, multiplied by {HEADROOM:g} so a shared runner passes, and read\n",
        "# by `bench.py --check` and crates/balaur_bench/tests/budgets.rs.\n\n",
        "[ceiling_ns]\n",
    ]
    lines += [f'"{name}" = {round(ns * HEADROOM)}\n' for name, ns in results.items()]
    BUDGETS.write_text("".join(lines), encoding="utf-8")
    print(f"wrote {BUDGETS.relative_to(ROOT)} ({len(results)} ceilings)")


def over_budget(results):
    """Benchmarks past their ceiling, and the ones nothing is gating."""
    ceilings = budgets()
    over = [(n, ns, ceilings[n]) for n, ns in results.items() if ns > ceilings.get(n, ns)]
    return over, sorted(set(results) - set(ceilings))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="fewer samples; noisier but ~1 min")
    ap.add_argument("--bench", help="only this bench target (scripting, engine)")
    ap.add_argument("--no-run", action="store_true", help="report the last run")
    ap.add_argument("--check", action="store_true", help="fail if a result is over budget")
    ap.add_argument("--record", action="store_true", help="rewrite the budgets from this run")
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

    if args.record:
        record(results)
    if args.check:
        over, ungated = over_budget(results)
        for name in ungated:
            print(f"no ceiling for {name}; run --record to add one")
        for name, ns, ceiling in over:
            print(f"OVER BUDGET  {name}: {human(ns)} against a {human(ceiling)} ceiling")
        if over:
            print(f"\n{len(over)} benchmark(s) over budget")
            return 1
        print(f"{len(results) - len(ungated)} benchmark(s) within budget")
    return 0


if __name__ == "__main__":
    sys.exit(main())
