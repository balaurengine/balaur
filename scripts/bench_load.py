#!/usr/bin/env python3
"""What one kind of node, or one shape of script, costs a frame.

The cases live in `examples/benchmark/scripts/cases_kinds.rn` and
`cases_scripts.rn`: N of one thing, built once, then every frame measured
while they sit there. This runs them and prints the table.

Two numbers matter and they are not the same. Wall milliseconds is what a
player feels and moves with the machine and its load. Instructions per node is
what the interpreter did, is identical everywhere, and is the one a baseline
should hold.

Nothing here runs in CI: a shared runner times a frame badly, and these are
for deciding what to optimise, not for gating a change.

  scripts/bench_load.py                  # every case
  scripts/bench_load.py --only script    # the script shapes
  scripts/bench_load.py --only kind/shape2d kind/text2d
  scripts/bench_load.py --json out.json  # keep the raw readings
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "benchmark"
# The case builds in chunks of 1500 a frame and then measures, so the cap has
# to cover both. Reached only when a case never finishes, which is a bug.
FRAME_CAP = 3000


def binary() -> Path:
    path = ROOT / "target" / "release" / "balaur"
    if not path.exists():
        print(
            "no target/release/balaur — run "
            "`cargo build --release -p balaur_cli --features window --bin balaur`",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return path


def keys(only: list[str]) -> list[str]:
    """Every load case the project declares, filtered by what was asked for."""
    listed = subprocess.run(
        [str(binary()), "run", str(PROJECT), "--headless", "--frames", "2",
         "--", "--list"],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    found = [
        line[5:].strip()
        for line in listed.stdout.splitlines()
        if line.startswith("CASE ")
    ]
    cases = [k for k in found if k.startswith(("kind/", "script/"))]
    if not only:
        return cases
    return [k for k in cases if any(k == want or k.startswith(want) for want in only)]


def run_case(key: str, steps: int, warmup: int, offscreen: bool,
             count: int | None = None) -> dict | None:
    """One case in its own process, so a warm allocator cannot flatter the next."""
    argv = [
        str(binary()), "run", str(PROJECT),
        "--offscreen" if offscreen else "--headless",
        "--frames", str(FRAME_CAP), "--", f"--case={key}",
        f"--steps={steps}", f"--warmup={warmup}",
    ]
    if count:
        argv.append(f"--count={count}")
    out = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, check=False)
    for line in out.stdout.splitlines():
        if line.startswith("BENCH "):
            return json.loads(line[6:])
    print(f"  {key}: no result", file=sys.stderr)
    for line in (out.stderr or "").splitlines()[:4]:
        print(f"    {line}", file=sys.stderr)
    return None


def median(result: dict, name: str) -> float:
    held = result.get(name)
    if isinstance(held, dict):
        return float(held.get("p50_ms", 0.0))
    return 0.0


def row(key: str, result: dict) -> str:
    count = max(1, int(result.get("count", 1)))
    per_node = median(result, "instructions") / count
    return (
        f"{key:24} {count:>7} "
        f"{median(result, 'wall_ms'):8.2f} {median(result, 'frame_ms'):8.2f} "
        f"{median(result, 'script_ms'):8.2f} {median(result, 'mirror_ms'):8.2f} "
        f"{median(result, 'ui_ms'):8.2f} "
        f"{median(result, 'render_cpu_ms'):8.2f} {median(result, 'render_gpu_ms'):8.2f} "
        f"{per_node:9.1f}"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--only", nargs="*", default=[], help="case keys or prefixes")
    ap.add_argument("--steps", type=int, default=120, help="frames measured")
    ap.add_argument("--warmup", type=int, default=30, help="frames settled first")
    ap.add_argument("--headless", action="store_true",
                    help="no renderer, so the render columns read zero")
    ap.add_argument("--count", type=int, help="override how many each case builds")
    ap.add_argument("--json", help="write every reading here")
    args = ap.parse_args()

    wanted = keys(args.only)
    if not wanted:
        print("no load cases matched", file=sys.stderr)
        return 1

    print(
        f"{'case':24} {'count':>7} {'wall':>8} {'frame':>8} {'script':>8} "
        f"{'mirror':>8} {'ui':>8} {'r.cpu':>8} {'r.gpu':>8} {'instr/n':>9}"
    )
    results = {}
    for key in wanted:
        result = run_case(key, args.steps, args.warmup, not args.headless, args.count)
        if result is None:
            continue
        results[key] = result
        print(row(key, result))
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
