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


def median_run(key, args):
    """`--repeats` runs of one case, keeping the one whose headline number is
    the median: a background process that lands on one run costs that run,
    not the result."""
    runs = [r for r in (run_case(key, args.steps, args.warmup) for _ in range(max(1, args.repeats))) if r]
    if not runs:
        return None
    headline = "loop_ms" if runs[0]["dimensions"] == "nodes" else "step_ms"
    runs.sort(key=lambda r: r[headline]["p50_ms"])
    return runs[len(runs) // 2]


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


def quickest(row):
    """The index of the smallest number in a row of optional milliseconds."""
    best = None
    for i, value in enumerate(row):
        if value is not None and (best is None or value < row[best]):
            best = i
    return best


def physics_table(dim, results, godot, shots):
    """One table for a dimension: a row per case, a column per engine, the
    quickest in bold, the case's picture in its first cell."""
    engines = [e for e in GODOT_ENGINES if any((dim, n, e) in godot for n in PHYSICS)]
    header = ["Balaur"] + [GODOT_ENGINES[e] for e in engines]
    lines = ["| | " + " | ".join(header) + " |", "| --- |" + " ---: |" * len(header)]
    rows = 0
    for name in PHYSICS:
        found = results.get(f"{dim}/{name}")
        values = [found["step_ms"]["p50_ms"] if found else None]
        values += [
            godot[(dim, name, e)]["step_ms"]["p50_ms"] if (dim, name, e) in godot else None
            for e in engines
        ]
        if all(v is None for v in values):
            continue
        best = quickest(values)
        cells = []
        for i, value in enumerate(values):
            if value is None:
                cells.append("—")
            elif i == best:
                cells.append(f"**{value:.2f} ms**")
            else:
                cells.append(f"{value:.2f} ms")
        label = f"**{name}**"
        if found:
            label += f"<br />{found['body_count']} bodies"
            if found["joint_count"]:
                label += f", {found['joint_count']} joints"
        if (shots / f"{dim}_{name}.png").exists():
            label += f"<br />![{name}](/{IMAGES}/{dim}_{name}.png)"
        lines.append(f"| {label} | " + " | ".join(cells) + " |")
        rows += 1
    lines.append("")
    return lines if rows else []


def nodes_table(results, godot):
    lines = [
        "Milliseconds for the whole loop, lower is better. Godot's numbers are "
        "its own published run, on a 12th-gen i5.\n",
        "| operation | Balaur | Godot |",
        "| --- | ---: | ---: |",
    ]
    for name in NODES:
        found = results.get(f"nodes/{name}")
        if not found:
            continue
        lines.append(f"| `{name}` | {ms(found['loop_ms']['p50_ms'])} ms | {ms(godot.get(name))} ms |")
    lines.append("")
    return lines


def conclusion(results, godot):
    """One sentence: how many cases Balaur is quickest on, and the worst gap."""
    won, total, worst = 0, 0, 0.0
    for dim in ("3d", "2d"):
        for name in PHYSICS:
            found = results.get(f"{dim}/{name}")
            if not found:
                continue
            ours = found["step_ms"]["p50_ms"]
            others = [
                godot[(dim, name, e)]["step_ms"]["p50_ms"]
                for e in GODOT_ENGINES if (dim, name, e) in godot
            ]
            if not others or ours <= 0:
                continue
            total += 1
            if ours <= min(others):
                won += 1
            else:
                worst = max(worst, ours / min(others))
    if not total:
        return None
    if won == total:
        return f"Balaur is the quickest engine on all {total} physics cases.\n"
    return (
        f"Balaur is the quickest engine on {won} of {total} physics cases, and "
        f"at worst {worst:.2f}x the quickest on the rest.\n"
    )


def report(results, godot, nodes, args, shots):
    version = godot_version(args.godot_results) or "4.7"
    lines = [
        "<!-- Written by scripts/bench_compare.py from a real run. -->\n",
        "# Benchmarks\n",
        f"Balaur `{commit()}` and Godot {version} on {machine()}, "
        f"{date.today().isoformat()}: the scenes of the [godot-rapier benchmark "
        f"suite]({SUITE_REPO}) ([post]({SUITE_POST}), [docs]({SUITE_DOCS})), "
        f"body for body, {args.steps} timed steps at 60 Hz after a settle. "
        "Median physics tick in milliseconds, lower is better.\n",
    ]
    for dim in ("3d", "2d"):
        table = physics_table(dim, results, godot, shots)
        if not table:
            continue
        lines.append(f"## {dim.upper()}\n")
        if (shots / f"chart_{dim}.svg").exists():
            lines.append(f"![{dim.upper()} chart](/{IMAGES}/chart_{dim}.svg)\n")
        lines += table
    if any(k.startswith("nodes/") for k in results):
        lines.append("## Nodes\n")
        lines += nodes_table(results, nodes)
    summary = conclusion(results, godot)
    if summary:
        lines.append(summary)
    lines += [
        "## Running it\n",
        "```bash\n"
        "cargo build --release -p balaur_cli --features window --bin balaur\n"
        "python3 scripts/bench_compare.py --shots --godot-results <benchmarks-repo>/results\n"
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
    ap.add_argument("--repeats", type=int, default=1,
                    help="runs per case; the one with the median tick is kept, as the Godot driver does")
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
            found = median_run(key, args)
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
