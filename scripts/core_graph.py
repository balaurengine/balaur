#!/usr/bin/env python3
"""The core split's `use` graph (docs/PLAN-core-split.md): every `crate::`
path each balaur_core module uses, and the edges that cross the crate table.

A crate may use the kernel and crates of a lower rank; crates of one rank
never meet. Prints each edge against that, and the lines in each crate.

Usage: core_graph.py [repo-root]
"""
import collections
import pathlib
import re
import sys

PLAN = {
    "geometry": "primitive csg geometry2d triangulate path stroke heightfield voxels skeleton tiles",
    "asset": "pixels texture_asset glb glb_material import mesh",
    "net": "rollback rollback_api netsession transport",
    "project": "pack desktop desktop_api standalone batteries_api",
    "debug": "dap debugger_api profile_api debug_lines",
}
RANK = {"kernel": 0, "geometry": 1, "net": 1, "asset": 2, "project": 3, "debug": 3}


def main() -> None:
    repo = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    root = repo / "crates/balaur_core/src"
    crate_of = {m: c for c, ms in PLAN.items() for m in ms.split()}
    uses: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    lines: collections.Counter = collections.Counter()
    for f in root.rglob("*.rs"):
        module = f.relative_to(root).parts[0].removesuffix(".rs")
        text = f.read_text()
        lines[module] += text.count("\n")
        for used in re.findall(r"\bcrate::([a-z_0-9]+)", text):
            if used != module:
                uses[module][used] += 1

    def home(m: str) -> str:
        return crate_of.get(m, "kernel")

    modules = sorted(m for m in lines if m != "lib")
    for crate in ["kernel", *PLAN]:
        mine = [m for m in modules if home(m) == crate]
        print(f"{crate:9} {sum(lines[m] for m in mine):6} lines, {len(mine)} modules")
    print("\nedges against the order:")
    for m in modules:
        for used, n in sorted(uses[m].items()):
            a, b = home(m), home(used)
            if a == b or used not in lines:
                continue
            if RANK[b] > RANK[a] or (RANK[a] == RANK[b] and a != b):
                print(f"  {m} [{a}] -> {used} [{b}] x{n}")


if __name__ == "__main__":
    main()
