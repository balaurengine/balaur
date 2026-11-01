#!/usr/bin/env python3
"""What a player's first launch costs against the launches after it, on macOS.

Metal keeps compiled shaders per app, so each trial runs the binary from a throwaway
bundle with a new identifier: its first launch starts with an empty disk cache, and
its later launches find what that one compiled. Running a binary with no bundle says
nothing about either, since those seem to share one cache.

  python3 scripts/first_launch.py examples/hello examples/interface
  python3 scripts/first_launch.py --binary target/release/balaur --trials 3 examples/objects

Each row is the minimum of its launches, in milliseconds, and `--timings` is where
every number comes from. The load average is printed because a machine doing other
work compiles shaders several times slower: compare two runs only at a similar load.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import time

PLIST = """<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleExecutable</key><string>balaur</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"""

ROW = re.compile(r"^(.+?)\s+([\d.]+) ms$")
SHADERS = re.compile(r"^shaders \((\d+) modules, (\d+) pipelines\)$")
LATER = re.compile(r"^later shaders: (\d+) modules, (\d+) pipelines, ([\d.]+) ms$")


def scratch_bundle(binary, identifier, root):
    """A bundle around `binary`, which stays one file on disk where it can."""
    bundle = os.path.join(root, identifier + ".app")
    macos = os.path.join(bundle, "Contents", "MacOS")
    os.makedirs(macos)
    with open(os.path.join(bundle, "Contents", "Info.plist"), "w", encoding="utf8") as plist:
        plist.write(PLIST.format(identifier=identifier))
    executable = os.path.join(macos, "balaur")
    try:
        os.link(binary, executable)
    except OSError:
        shutil.copy2(binary, executable)
    return bundle, executable


def launch(executable, project, frames):
    """The boot table of one run, as `{row: ms}`."""
    run = subprocess.run(
        [executable, "run", project, "--offscreen", "--frames", str(frames), "--timings"],
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        sys.exit(f"{project} exited {run.returncode}:\n{run.stderr[-2000:]}")
    table = {}
    for line in run.stdout.splitlines():
        later = LATER.match(line)
        if later:
            table["later shaders"] = float(later.group(3))
            break
        row = ROW.match(line)
        if not row:
            continue
        shaders = SHADERS.match(row.group(1))
        name = "shaders" if shaders else row.group(1)
        table[name] = float(row.group(2))
        if shaders:
            table["pipelines"] = float(shaders.group(2))
    return table


def least(runs, name):
    return min(run.get(name, 0.0) for run in runs)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("projects", nargs="+")
    parser.add_argument("--binary", default="target/debug/balaur")
    parser.add_argument("--frames", type=int, default=120)
    parser.add_argument("--trials", type=int, default=2, help="first launches per project")
    parser.add_argument("--again", type=int, default=2, help="later launches per first one")
    args = parser.parse_args()
    if sys.platform != "darwin":
        sys.exit("this measures Metal's per-app cache, so it runs on macOS")

    binary = os.path.abspath(args.binary)
    caches = subprocess.run(
        ["getconf", "DARWIN_USER_CACHE_DIR"], capture_output=True, text=True, check=True
    ).stdout.strip()
    root = os.path.join(os.path.dirname(binary), "first_launch_bundles")
    os.makedirs(root, exist_ok=True)

    print(f"load {os.getloadavg()[0]:.0f}, {binary}")
    columns = ("to first frame", "device", "shaders", "pipelines", "later shaders")
    header = "".join(f"{name:>16}" for name in columns)
    print(f"{'':32}{'':8}{header}")
    for project in args.projects:
        first, later = [], []
        for trial in range(args.trials):
            identifier = f"org.balaurengine.firstlaunch.t{int(time.time())}x{trial}"
            bundle, executable = scratch_bundle(binary, identifier, root)
            try:
                first.append(launch(executable, project, args.frames))
                later += [launch(executable, project, args.frames) for _ in range(args.again)]
            finally:
                shutil.rmtree(bundle, ignore_errors=True)
                shutil.rmtree(os.path.join(caches, identifier), ignore_errors=True)
        for label, runs in (("first", first), ("later", later)):
            cells = "".join(f"{least(runs, name):>16.1f}" for name in columns)
            print(f"{project:32}{label:8}{cells}")
    shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    main()
