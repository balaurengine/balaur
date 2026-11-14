#!/usr/bin/env python3
"""Each example's picture against the one committed in `scripts/pictures/`.

    scripts/pictures.py [--update] [example...]

An example runs offscreen at a fixed tick and its last frame is compared with
the committed PNG. `--update` writes the committed ones instead. A picture is
the GPU's, so the committed set is the release Mac's and this is no CI job.
"""

import argparse
import os
import pathlib
import shutil
import subprocess
import sys

from PIL import Image, ImageChops

ROOT = pathlib.Path(__file__).resolve().parent.parent
KEPT = ROOT / "scripts" / "pictures"
OUT = ROOT / "target" / "pictures"
FRAMES = 200
SIZE = "800x500"
# A channel further off than this is a pixel that moved, and more of those
# than this share of the frame is a picture that changed.
CHANNEL = 24
SHARE = 0.005


def binary():
    """A copy of the build, so a `cargo` run meanwhile cannot swap it out."""
    built = os.environ.get("BALAUR_BIN")
    if not built:
        subprocess.run(["cargo", "editor", "-q"], cwd=ROOT, check=True)
        built = ROOT / os.environ.get("CARGO_TARGET_DIR", "target") / "debug" / "balaur"
    kept = OUT / "bin" / "balaur"
    kept.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(built, kept)
    return kept


def shoot(run, example, into):
    """Run `example` and answer what went wrong, or nothing."""
    env = dict(os.environ, BALAUR_DATA_DIR=str(OUT / "data"))
    args = [str(run), "run", f"examples/{example}", "--offscreen", "--fixed-tick"]
    args += ["--size", SIZE, "--frames", str(FRAMES), "--shot", str(into)]
    done = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, check=False)
    log = done.stdout + done.stderr
    if done.returncode != 0:
        return f"exited {done.returncode}"
    if any(word in log for word in ("ERROR", "WARN", "panicked")):
        return "logged a warning or an error"
    if not into.exists():
        return "saved no picture"
    return None


def moved(kept, shot):
    """The share of pixels that differ past `CHANNEL`, and where they are."""
    was = Image.open(kept).convert("RGB")
    now = Image.open(shot).convert("RGB")
    if was.size != now.size:
        return 1.0, None
    apart = ImageChops.difference(was, now).convert("L").point(lambda v: 255 if v > CHANNEL else 0)
    count = apart.histogram()[255]
    return count / (was.size[0] * was.size[1]), apart


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--update", action="store_true", help="write the committed pictures")
    parser.add_argument("examples", nargs="*")
    asked = parser.parse_args()
    examples = asked.examples or sorted(
        path.parent.name for path in (ROOT / "examples").glob("*/project.toml")
    )
    run = binary()
    KEPT.mkdir(parents=True, exist_ok=True)
    failed = []
    for example in examples:
        shot = OUT / f"{example}.png"
        shot.unlink(missing_ok=True)
        wrong = shoot(run, example, shot)
        kept = KEPT / f"{example}.png"
        if wrong is None and asked.update:
            shutil.copy2(shot, kept)
            print(f"{example:<16} written")
            continue
        if wrong is None and not kept.exists():
            wrong = "has no committed picture; run with --update"
        if wrong is None:
            share, apart = moved(kept, shot)
            if share > SHARE:
                wrong = f"{share:.1%} of the picture moved; see {shot.relative_to(ROOT)}"
                if apart is not None:
                    apart.save(OUT / f"{example}.moved.png")
        if wrong:
            failed.append(example)
            print(f"{example:<16} FAILED: {wrong}")
        else:
            print(f"{example:<16} ok")
    if failed:
        print(f"{len(failed)} failed: {' '.join(failed)}")
        sys.exit(1)


if __name__ == "__main__":
    main()
