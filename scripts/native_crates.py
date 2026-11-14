#!/usr/bin/env python3
"""Charge every function of a native binary to the crate that defines it.

Usage: native_crates.py binary [--top N]

Reads the symbol table through `nm`, or the one `NM` names, which a release
build keeps. A function
runs to the next symbol's address, since Mach-O records no sizes; padding
between functions is charged to the one before it. A universal binary is
read for its arm64 slice alone.
"""
import os
import shutil
import subprocess
import sys
from collections import defaultdict

from wasm_crates import crate_of

CODE = "tT"


def slices(path):
    if not shutil.which("lipo"):
        return ""
    return subprocess.run(["lipo", "-archs", path], capture_output=True, text=True).stdout.strip()


def functions(path):
    arch = ["-arch", "arm64"] if " " in slices(path) else []
    nm = os.environ.get("NM") or "nm"
    listing = subprocess.run([nm, "--defined-only", "-n", "-C", *arch, path], check=True,
                             capture_output=True, text=True).stdout
    rows = []
    for line in listing.splitlines():
        parts = line.split(maxsplit=2)
        if len(parts) == 3:
            rows.append((int(parts[0], 16), parts[1], parts[2]))
    for (address, kind, name), (after, _, _) in zip(rows, rows[1:]):
        if kind in CODE and after > address:
            yield after - address, name


def main():
    path = sys.argv[1]
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    per = defaultdict(int)
    for size, name in functions(path):
        per[crate_of(name)] += size
    mib = lambda b: b / 1048576
    with open(path, "rb") as f:
        whole = len(f.read())
    print(f"file {mib(whole):.2f} MiB: code {mib(sum(per.values())):.2f}")
    for crate, size in sorted(per.items(), key=lambda kv: -kv[1])[:top]:
        print(f"{mib(size):7.3f} {crate}")


if __name__ == "__main__":
    main()
