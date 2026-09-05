#!/usr/bin/env python3
"""Run examples/benchmark headless, merge Godot's results, write a report.

The suite mirrors two published ones case for case, so a row here is the same
scene measured the same way: the physics cases come from
github.com/Ughuuu/benchmarks-repo (the numbers behind godot-rapier v0.35) and
the scene-tree cases from godotengine/godot-benchmarks.

  python3 scripts/bench_compare.py --godot-results ~/Documents/appsinacup/benchmarks-repo/results
  python3 scripts/bench_compare.py --cases 3d/pyramid --quick

Nothing here is generated in CI. A shared runner's numbers are noise; the
committed report says which machine it came from, and budgets.toml is what
gates a regression.
"""
import argparse
import json
import platform
import subprocess
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "benchmark"
REPORT = ROOT / "docs" / "BENCHMARKS.md"
FRAME_MS = 1000.0 / 60.0

PHYSICS = ["pyramid", "mixed_pile", "joint_grid", "smash", "query_storm", "drop"]
NODES = [
    "add_children",
    "delete_children_in_order",
    "delete_children_reverse",
    "delete_children_random",
    "get_node",
]
# Enough frames for the build, the settle and the timed window, and a ceiling
# so a case that wedges costs one result rather than the whole sweep.
FRAME_CAP = 4000

# What each Godot engine is called in the suite's results/ tree, and what the
# report calls it.
GODOT_ENGINES = {
    "Rapier2D": "Godot Rapier 2D", "Box2D": "Godot Box2D v3",
    "GodotPhysics2D": "Godot Physics 2D", "Rapier3D": "Godot Rapier 3D",
    "Jolt_Physics": "Godot Jolt", "Box3D_Physics": "Godot Box3D",
    "GodotPhysics3D": "Godot Physics 3D",
}
SUITE_REPO = "https://github.com/Ughuuu/benchmarks-repo"
SUITE_POST = "https://godot.rapier.rs/blog/v0-35-0"
SUITE_DOCS = "https://godot.rapier.rs/docs/documentation/performance"
# Where the site keeps the pictures the report points at, under static/.
IMAGES = "img/benchmarks"


def cases(only, dims):
    out = []
    for dim in dims:
        names = NODES if dim == "nodes" else PHYSICS
        out += [f"{dim}/{name}" for name in names]
    return [c for c in out if not only or c in only]


def binary():
    """The release binary. A debug one is minutes per case and worth nothing."""
    path = ROOT / "target" / "release" / "balaur"
    if not path.exists():
        print(
            "no target/release/balaur — run "
            "`cargo build --release -p balaur_cli --bin balaur`",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return path


def run_case(key, steps, warmup):
    """One case in its own process, so a warm allocator cannot flatter the next."""
    argv = [
        str(binary()), "run", str(PROJECT), "--headless", "--fixed-tick",
        "--frames", str(FRAME_CAP), "--", f"--case={key}",
        f"--steps={steps}", f"--warmup={warmup}",
    ]
    out = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, check=False)
    for line in out.stdout.splitlines():
        if line.startswith("BENCH "):
            return json.loads(line[6:])
    print(f"  {key}: no result", file=sys.stderr)
    for line in (out.stderr or "").splitlines()[:4]:
        print(f"    {line}", file=sys.stderr)
    return None


def shoot_case(key, warmup, path):
    """One picture of a case, offscreen, at the first timed tick.

    A separate run from the timed one: drawing every body is the viewer's
    cost, not the measurement's, and the timed run keeps shapes off.
    """
    argv = [
        str(binary()), "run", str(PROJECT), "--offscreen", "--fixed-tick",
        "--frames", str(FRAME_CAP), "--", f"--case={key}", "--steps=1",
        f"--warmup={warmup}", f"--shot={path}",
    ]
    out = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, check=False)
    if not Path(path).exists():
        print(f"  {key}: no screenshot", file=sys.stderr)
        for line in (out.stderr or "").splitlines()[-4:]:
            print(f"    {line}", file=sys.stderr)
        return False
    return True


def godot_results(directory):
    """The suite's own results, keyed by (dim, case, engine)."""
    out = {}
    if not directory:
        return out
    root = Path(directory).expanduser()
    for path in sorted(root.rglob("*.json")):
        if path.name == "environment.json":
            continue
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        if "step_ms" not in data:
            continue
        dim = path.relative_to(root).parts[0]
        engine = path.parent.name
        out.setdefault((dim, data["case"], engine), []).append(data)
    # Several repeats of one combination: the median run, by its own median.
    return {k: sorted(v, key=lambda d: d["step_ms"]["p50_ms"])[len(v) // 2] for k, v in out.items()}


def godot_nodes(path):
    """The `Scene Nodes` rows of a godot-benchmarks results JSON."""
    out = {}
    if not path:
        return out
    text = Path(path).expanduser().read_text()
    data, _ = json.JSONDecoder().raw_decode(text[text.index('{"benchmarks"'):])
    wanted = {
        "Add Children Without Name": "add_children",
        "Delete Children In Order": "delete_children_in_order",
        "Delete Children Reverse Order": "delete_children_reverse",
        "Delete Children Random Order": "delete_children_random",
        "Get Node": "get_node",
    }
    for row in data["benchmarks"]:
        name = wanted.get(row["name"])
        if name and "Scene Nodes" in row["category"]:
            release = row["results"].get("cpu_release") or {}
            if "time" in release:
                out[name] = release["time"]
    out["_system"] = data.get("system", {})
    out["_engine"] = data.get("engine", {})
    return out


def machine():
    name = platform.processor() or platform.machine()
    if sys.platform == "darwin":
        name = subprocess.run(
            ["sysctl", "-n", "machdep.cpu.brand_string"],
            capture_output=True, text=True, check=False,
        ).stdout.strip() or name
    cores = subprocess.run(
        ["getconf", "_NPROCESSORS_ONLN"], capture_output=True, text=True, check=False
    ).stdout.strip()
    return f"{name}, {cores} cores, {platform.system()} {platform.release()}"


def commit():
    return subprocess.run(
        ["git", "rev-parse", "--short=9", "HEAD"], cwd=ROOT,
        capture_output=True, text=True, check=False,
    ).stdout.strip() or "unknown"


def godot_version(directory):
    for path in sorted(Path(directory).expanduser().rglob("*.json")) if directory else []:
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        if "godot_version" in data:
            return data["godot_version"]
    return None


def ms(value):
    return f"{value:.2f}" if value is not None else "—"


def best_godot(dim, name, godot):
    """The quickest Godot engine on this case, and what it took."""
    found = [
        (label, godot[(dim, name, engine)]["step_ms"]["p50_ms"])
        for engine, label in GODOT_ENGINES.items()
        if (dim, name, engine) in godot
    ]
    return min(found, key=lambda row: row[1]) if found else None


def chart(dim, results, godot):
    """A bar chart of every engine on every case of one dimension, as SVG.

    Drawn by hand rather than through a plotting library, so the only
    dependency the driver has is the engine it measures.
    """
    groups = []
    for name in PHYSICS:
        bars = []
        found = results.get(f"{dim}/{name}")
        if found:
            bars.append(("Balaur", found["step_ms"]["p50_ms"], True))
        for engine, label in GODOT_ENGINES.items():
            entry = godot.get((dim, name, engine))
            if entry:
                bars.append((label, entry["step_ms"]["p50_ms"], False))
        if bars:
            groups.append((name, bars))
    if not groups:
        return None
    longest = max(value for _, bars in groups for _, value, _ in bars)
    # Room after the longest bar for its label: "84.90 ms · Godot Physics 3D".
    left, bar_w, bar_h, gap, top = 150, 460, 14, 3, 34
    rows = sum(len(bars) for _, bars in groups)
    height = top + rows * (bar_h + gap) + len(groups) * 14 + 16
    width = left + bar_w + 330
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" font-family="system-ui, sans-serif" font-size="12">',
        f'<rect width="{width}" height="{height}" fill="#ffffff"/>',
        f'<text x="{left}" y="20" fill="#222" font-size="13" font-weight="600">'
        f'{dim.upper()}: median physics tick, milliseconds, lower is better</text>',
    ]
    y = top
    for name, bars in groups:
        out.append(f'<text x="{left - 8}" y="{y + 11}" fill="#222" text-anchor="end" '
                   f'font-weight="600">{name}</text>')
        for label, value, ours in bars:
            w = max(2.0, bar_w * value / longest)
            fill = "#2f6fed" if ours else "#b9bec7"
            out.append(f'<rect x="{left}" y="{y}" width="{w:.1f}" height="{bar_h}" '
                       f'fill="{fill}" rx="2"/>')
            out.append(f'<text x="{left + w + 6:.1f}" y="{y + 11}" fill="#222">'
                       f'{value:.2f} ms · {label}</text>')
            y += bar_h + gap
        y += 14
    out.append("</svg>")
    return "\n".join(out) + "\n"


def summary_table(results, godot):
    """The headline: our tick beside the quickest Godot engine's, per case."""
    lines = [
        "| case | bodies | Balaur | quickest in Godot | that engine's tick, over ours |",
        "| --- | ---: | ---: | --- | ---: |",
    ]
    rows = 0
    for dim in ("3d", "2d"):
        for name in PHYSICS:
            found = results.get(f"{dim}/{name}")
            if not found:
                continue
            ours = found["step_ms"]["p50_ms"]
            other = best_godot(dim, name, godot)
            against = f"{other[0]} {other[1]:.2f} ms" if other else "—"
            ratio = f"{other[1] / ours:.2f}x" if other and ours > 0 else "—"
            lines.append(
                f"| `{dim}/{name}` | {found['body_count']} | {ours:.2f} ms | "
                f"{against} | {ratio} |"
            )
            rows += 1
    lines.append("")
    return lines if rows else []


def physics_table(dim, results, godot, shots):
    """One table per case: us, rapier inside us, and every Godot engine."""
    lines = []
    for name in PHYSICS:
        found = results.get(f"{dim}/{name}")
        rows = []
        if found:
            step = found["step_ms"]
            rows.append(("**Balaur**", ms(step["p50_ms"]), ms(step["p99_ms"])))
        for engine, label in GODOT_ENGINES.items():
            entry = godot.get((dim, name, engine))
            if entry:
                rows.append((label, ms(entry["step_ms"]["p50_ms"]), ms(entry["step_ms"]["p99_ms"])))
        if not rows:
            continue
        rows.sort(key=lambda r: float(r[1]) if r[1] != "—" else 1e9)
        lines.append(f"### `{name}` ({dim})\n")
        if (shots / f"{dim}_{name}.png").exists():
            lines.append(f"![{dim} {name} in Balaur](/{IMAGES}/{dim}_{name}.png)\n")
        if found:
            lines.append(
                f"{found['body_count']} bodies"
                + (f", {found['joint_count']} joints" if found["joint_count"] else "")
                + (f", {found['static_count']} static" if found["static_count"] > 1 else "")
                + f". {found['steps']} timed steps after {found['warmup']}.\n"
            )
        lines.append("| engine | step p50 | step p99 |")
        lines.append("| --- | ---: | ---: |")
        for row in rows:
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")
        if found and found["extra_metrics"]:
            pairs = ", ".join(
                f"{k} {v:.4g}" if isinstance(v, float) else f"{k} {v}"
                for k, v in sorted(found["extra_metrics"].items())
            )
            lines.append(f"Balaur: {pairs}. Fingerprint `{found['fingerprint']}`.\n")
    return lines


def nodes_table(results, godot):
    lines = ["### Scene tree\n"]
    lines.append(
        "Milliseconds for the whole loop, lower better. Godot's column is its "
        "own published run on another machine, so it says which operations are "
        "in a different class, not how the two machines compare.\n"
    )
    lines.append("| operation | Balaur p50 | Balaur min | Godot release |")
    lines.append("| --- | ---: | ---: | ---: |")
    for name in NODES:
        found = results.get(f"nodes/{name}")
        if not found:
            continue
        lines.append(
            f"| `{name}` | {ms(found['loop_ms']['p50_ms'])} | "
            f"{ms(found['loop_ms']['min_ms'])} | {ms(godot.get(name))} |"
        )
    lines.append("")
    lines.append(
        "`move_child` has no row: the scene tree has no sibling-reorder "
        "operation to measure.\n"
    )
    lines.append(
        "Destroying is two orders of magnitude off, and it is the scene tree, "
        "not the script: `scene::free_subtree` unlinks each node from its "
        "parent by scanning every sibling, so freeing a flat container of "
        "fifty thousand is quadratic. Adding and looking up are within a "
        "small factor of Godot's on a faster machine.\n"
    )
    return lines


def report(results, godot, nodes, args, shots):  # noqa: C901
    ran = [r for r in results.values() if r and r["dimensions"] != "nodes"]
    lines = [
        "<!-- Written by scripts/bench_compare.py from a real run. -->\n",
        "# Benchmarks\n",
        f"Balaur `{commit()}` on {machine()}, {date.today().isoformat()}.\n",
        "Every case is a scene from the [godot-rapier benchmark suite]"
        f"({SUITE_REPO}), the one behind [its v0.35 post]({SUITE_POST}) and "
        f"[its performance page]({SUITE_DOCS}), built body for body with the "
        "same counts, the same 60 Hz tick and the same window: a settle, then "
        f"{args.steps} timed steps, reported as the median and the 99th "
        "percentile of one tick.\n",
        "**step p50** is a whole physics tick: script `fixed_update`, the "
        "solver, and writing every simulated pose back to the scene tree — "
        "what the Godot suite calls `step_ms`. The profiler also records "
        "rapier's own step inside it; what is left over is the engine, and "
        "in the query cases the queries themselves, which run inside the "
        "script call.\n",
    ]
    if ran:
        # What is left of a tick once rapier and the case's own script are
        # taken out: the scene tree, the components, the pose write-back.
        overhead = [
            100.0 * (r["step_ms"]["p50_ms"] - r["physics_ms"]["p50_ms"]
                     - r["script_ms"]["p50_ms"]) / r["step_ms"]["p50_ms"]
            for r in ran if r["step_ms"]["p50_ms"] > 0
        ]
        if overhead:
            lines.append(
                f"Across the {len(overhead)} physics cases the engine itself "
                f"costs {min(overhead):.0f}% to {max(overhead):.0f}% of a "
                "tick; the rest is rapier, and in the query cases the "
                "script's own calls.\n"
            )
    for dim in ("3d", "2d"):
        if (shots / f"chart_{dim}.svg").exists():
            lines.append(f"![{dim.upper()} chart](/{IMAGES}/chart_{dim}.svg)\n")
    summary = summary_table(results, godot)
    if summary:
        lines.append("Median tick, lower better:\n")
        lines += summary
    version = godot_version(args.godot_results)
    if version:
        lines.append(
            f"The Godot columns are {version} with the addons the reference "
            "suite commits, run on this same machine.\n"
        )
    for dim in ("3d", "2d"):
        table = physics_table(dim, results, godot, shots)
        if table:
            lines.append(f"## {dim.upper()}\n")
            lines += table
    if any(k.startswith("nodes/") for k in results):
        lines.append("## Nodes\n")
        lines += nodes_table(results, nodes)
    lines += [
        "## What is not the same\n",
        "- Balaur's rapier is built without SIMD; the godot-rapier addon's is "
        "built with it. Same version, same `enhanced-determinism`, same "
        "threaded solver.\n",
        "- The worlds are statistically alike, not bit-identical: the scattered "
        "cases draw from each engine's own random stream, so a fingerprint "
        "compares two Balaur runs, never Balaur against Godot.\n",
        "- The 2D cases are the GDScript's coordinates with y negated, because "
        "Godot's 2D plane points down and ours points up. Distances, contacts "
        "and gravity are unchanged, and `length_unit` is 100 as godot-rapier's "
        "2D default is.\n",
        "- `mixed_pile`'s convex hull is one fixed size rather than a random "
        "one: a hull collider is built from a mesh asset and has no scale.\n",
        "- Godot's own suite runs its scene-tree cases on its CI machine, not "
        "this one.\n",
        "## Running it\n",
        "```bash\n"
        "cargo build --release -p balaur_cli --bin balaur\n"
        "python3 scripts/bench_compare.py --godot-results <benchmarks-repo>/results\n"
        "```\n",
        "One case on its own, or in the editor:\n",
        "```bash\n"
        "target/release/balaur run examples/benchmark --headless --fixed-tick "
        "-- --case=3d/pyramid\n"
        "target/release/balaur edit examples/benchmark\n"
        "```\n",
    ]
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", nargs="*", help="only these keys, e.g. 3d/pyramid")
    ap.add_argument("--dims", nargs="*", default=["3d", "2d", "nodes"])
    ap.add_argument("--steps", type=int, default=300)
    ap.add_argument("--warmup", type=int, default=60)
    ap.add_argument("--quick", action="store_true", help="60 steps; noisier, minutes not tens")
    ap.add_argument("--godot-results", help="the benchmarks-repo results/ directory")
    ap.add_argument("--godot-nodes", help="a godot-benchmarks results JSON")
    ap.add_argument("--out", default=str(REPORT))
    ap.add_argument("--site", default=str(ROOT.parent / "balaur-website"),
                    help="the website checkout; pictures land in its static/")
    ap.add_argument("--shots", action="store_true",
                    help="also take one screenshot per physics case, offscreen")
    ap.add_argument("--no-run", action="store_true", help="report from results.json")
    ap.add_argument("--json", default=str(ROOT / "target" / "bench-results.json"))
    args = ap.parse_args()
    if args.quick:
        args.steps, args.warmup = 60, 20

    raw = Path(args.json)
    results = {}
    if args.no_run and raw.exists():
        results = json.loads(raw.read_text())
    else:
        wanted = cases(args.cases, args.dims)
        for i, key in enumerate(wanted, 1):
            print(f"[{i}/{len(wanted)}] {key}", flush=True)
            found = run_case(key, args.steps, args.warmup)
            if found:
                results[key] = found
                if found["dimensions"] == "nodes":
                    print(f"    {found['loop_ms']['p50_ms']:.1f} ms for the loop")
                else:
                    print(
                        f"    step {found['step_ms']['p50_ms']:.2f} ms, "
                        f"rapier {found['physics_ms']['p50_ms']:.2f} ms, "
                        f"{100 * found['step_ms']['p50_ms'] / FRAME_MS:.0f}% of a frame"
                    )
        raw.parent.mkdir(parents=True, exist_ok=True)
        raw.write_text(json.dumps(results, indent=1) + "\n")

    if not results:
        print("nothing ran", file=sys.stderr)
        return 1
    shots = Path(args.site).expanduser() / "static" / IMAGES
    if args.shots:
        shots.mkdir(parents=True, exist_ok=True)
        for key in [k for k in results if not k.startswith("nodes/")]:
            dim, name = key.split("/", 1)
            print(f"shot {key}", flush=True)
            shoot_case(key, args.warmup, str(shots / f"{dim}_{name}.png"))
    godot = godot_results(args.godot_results)
    if shots.parent.exists():
        shots.mkdir(parents=True, exist_ok=True)
        for dim in ("3d", "2d"):
            drawn = chart(dim, results, godot)
            if drawn:
                (shots / f"chart_{dim}.svg").write_text(drawn)
    nodes = godot_nodes(args.godot_nodes)
    text = report(results, godot, nodes, args, shots)
    Path(args.out).write_text(text)
    print(f"wrote {Path(args.out).relative_to(ROOT)} ({len(results)} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
