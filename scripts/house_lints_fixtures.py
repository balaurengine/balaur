#!/usr/bin/env python3
"""The house lints' file-writing rules held to fixtures: each line a rule
must flag says so, `// caught: <rule>`, and every other line must pass.

A fixture is the rule's memory of the spellings it was widened for; a
change that loosens a regex fails here before a hand-made write slips into
the tree.

Usage:
  python3 scripts/house_lints_fixtures.py
"""
from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import house_lints as lints  # noqa: E402

RULES = ("in-place-write", "hand-made-partial")

FIXTURE = r'''
use std::fs::{read, write as put};
use std::fs::copy;

fn keep(path: &std::path::Path, other: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, b"x")?; // caught: in-place-write
    fs::copy(other, path)?; // caught: in-place-write
    put(path, b"x")?; // caught: in-place-write
    copy(other, path)?; // caught: in-place-write
    read(path)?;
    self.write(path);
    backend.write(path, b"x")?;
    backend.write_in_place(path, b"x")?; // caught: in-place-write
    let _f = std::fs::File::create(path)?; // caught: in-place-write
    let _w = std::fs::OpenOptions::new() // caught: in-place-write
        .write(true)
        .open(path)?;
    let _a = std::fs::OpenOptions::new()
        .write(true) // keep; see below
        .append(true)
        .open(path)?;
    let _n = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    let _r = std::fs::OpenOptions::new().read(true).open(path)?;
    let _o = std::fs::File::options().write(true).open(path)?; // caught: in-place-write
    let mut built = std::fs::OpenOptions::new(); // caught: in-place-write
    built.write(true);
    let _t = balaur_core::whole::private_options().create(true).truncate(true).open(path)?; // caught: in-place-write
    let _k = std::fs::File::create(path)?; // whole: a log, appended to after
    std::fs::rename(other, path)?; // caught: hand-made-partial
    let _p = path.with_extension("part"); // caught: hand-made-partial
    let _q = format!("{}.partial", path.display()); // caught: hand-made-partial
    let _s = "the part of it";
    balaur_core::whole::write(path, b"x")?;
    balaur_core::whole::Batch::new().write(path, b"x")?;
    Ok(())
}
'''


def flagged(text: str) -> dict[int, set[str]]:
    """The fixture's lines each rule flags, by line."""
    with tempfile.TemporaryDirectory() as root:
        lints.ROOT = Path(root)
        path = Path(root) / "crates" / "balaur_probe" / "src" / "probe.rs"
        path.parent.mkdir(parents=True)
        path.write_text(text, encoding="utf-8")
        ctx = lints.Context(set(), set(), set(), set())
        found: dict[int, set[str]] = {}
        for finding in lints.check_file(path, ctx):
            if finding.rule in RULES:
                found.setdefault(finding.line, set()).add(finding.rule)
        return found


def main() -> int:
    lines = FIXTURE.splitlines()
    wanted = {
        i: {m.group(1)}
        for i, line in enumerate(lines, start=1)
        if (m := re.search(r"// caught: ([\w-]+)", line))
    }
    found = flagged(FIXTURE)
    wrong = 0
    for i in sorted(set(wanted) | set(found)):
        if wanted.get(i, set()) != found.get(i, set()):
            wrong += 1
            print(f"line {i}: wanted {sorted(wanted.get(i, set()))}, "
                  f"flagged {sorted(found.get(i, set()))}: {lines[i - 1].strip()}")
    print(f"{len(wanted)} caught, {len(lines) - len(wanted)} passing, {wrong} wrong")
    return 1 if wrong else 0


if __name__ == "__main__":
    sys.exit(main())
