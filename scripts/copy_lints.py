#!/usr/bin/env python3
"""Copy lints for Balaur (docs/PLAN-copies.md).

A value is looked at through a reference and copied only where the copy is
the point. These are the shapes a copy that is not the point takes, each
found twice or more by the review that plan records:

  copy-per-item      `.clone()`, `.to_vec()` or `.to_owned()` inside the
                     closure of a `map`, `for_each`, `filter_map` or
                     `flat_map`, or `.cloned()` handed straight to a
                     `collect`: a copy an item, of what the items were lent
                     from. The shape that cost 1.9 s on a parsed document.
  copy-to-iterate    `for x in y.clone()`, `for x in y.to_vec()`.
  copy-to-borrow     `&x.clone()`, `&x.to_vec()`, `&x.to_owned()`.
  copy-out-of-a-cell `.borrow()…get(k).cloned()` and `.borrow().clone()`.

A copy that is the point says why on its line or in the comment above it,
starting `// copy:`; that is what a finding asks for when the copy stays.
The lint cannot see a type, so a cheap handle (an `Rc`, a `SmolStr`, an
`Entity`) inside a closure is a finding too: say `// copy: a handle`, or
`.map(Rc::clone)`, which reads as what it is.

Reported, never failing (`--reports`): a converter (`from_toml`,
`to_neutral`, …) handed `&local`, which is a second copy when `local` is
not read again; only a reader can tell.

The baseline, scripts/copy_lints.baseline, is the findings of the day the
lint was written, a line each. A finding not in it fails; so does a line of
it that no longer matches anything, so the file only shrinks.

Usage:
  python3 scripts/copy_lints.py                  # print findings past the baseline
  python3 scripts/copy_lints.py --fail-on-error  # and exit 1 on any
  python3 scripts/copy_lints.py --reports        # the reported shapes too
  python3 scripts/copy_lints.py --write-baseline # take today's findings as the baseline
"""
from __future__ import annotations

import argparse
import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "scripts" / "copy_lints.baseline"

COPY = r"\.(?:clone|to_vec|to_owned)\(\)"
# A closure handed to an iterator adapter that runs once an item.
PER_ITEM = re.compile(r"\.(?:map|for_each|filter_map|flat_map)\(\s*(?:move\s+)?\|")
COPY_CALL = re.compile(COPY)
CLONED_COLLECT = re.compile(r"\.cloned\(\)\s*\.collect\b")
TO_ITERATE = re.compile(rf"\bfor\s+.+\s+in\s+[^{{]*{COPY}\s*\{{")
TO_BORROW = re.compile(rf"&\s*[A-Za-z_][\w.]*(?:\([^()]*\))?{COPY}(?![.\w?\[])")
OUT_OF_A_CELL = re.compile(r"\.borrow\(\)(?:[^;]*\.get\([^()]*\)\.cloned\(\)|\.clone\(\))")
CONVERTED = re.compile(r"\b(?:from_neutral|to_neutral|dynamic|from_toml|to_toml|from_json|to_json)\(&[a-z_]+[,)]")
# The reason a copy stays, on its line or in the comment block above it.
WHY = "// copy:"
# How far above a finding its comment may start.
WHY_LINES = 6


def sources() -> list[Path]:
    out = []
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        rel = path.relative_to(ROOT).parts
        if "tests" in rel or "benches" in rel or "target" in rel:
            continue
        if path.name in ("tests.rs",) or path.name.endswith("_tests.rs"):
            continue
        out.append(path)
    return out


def code_of(line: str) -> str:
    """A line with its trailing comment cut; a string holding `//` is rare
    enough in a copy's line to leave."""
    at = line.find("//")
    return line if at < 0 else line[:at]


def explained(lines: list[str], at: int) -> bool:
    if WHY in lines[at]:
        return True
    for back in range(1, WHY_LINES + 1):
        if at - back < 0:
            return False
        above = lines[at - back].strip()
        if WHY in above:
            return True
        if not above.startswith("//"):
            return False
    return False


def closure_end(lines: list[str], at: int, start: int) -> int:
    """The line a call opened at `lines[at][start]` (its `(`) closes on."""
    depth = 0
    for number in range(at, min(at + 40, len(lines))):
        text = code_of(lines[number])
        for index, char in enumerate(text):
            if number == at and index < start:
                continue
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    return number
    return at


def findings_of(path: Path) -> tuple[list[tuple[int, str, str]], list[tuple[int, str, str]]]:
    """A file's findings and reports, each `(line, rule, the line's text)`."""
    lines = path.read_text().splitlines()
    found: dict[tuple[int, str], str] = {}
    reported: list[tuple[int, str, str]] = []
    for at, raw in enumerate(lines):
        # A file's own tests sit at its end.
        if raw.strip() == "#[cfg(test)]":
            break
        code = code_of(raw)
        if code.lstrip().startswith("//"):
            continue
        for opened in PER_ITEM.finditer(code):
            end = closure_end(lines, at, opened.start() + code[opened.start():].index("("))
            for inside in range(at, end + 1):
                text = code_of(lines[inside])
                begin = opened.end() if inside == at else 0
                if COPY_CALL.search(text, begin):
                    found.setdefault((inside, "copy-per-item"), lines[inside])
        if CLONED_COLLECT.search(code):
            found.setdefault((at, "copy-per-item"), raw)
        if TO_ITERATE.search(code):
            found.setdefault((at, "copy-to-iterate"), raw)
        if TO_BORROW.search(code):
            found.setdefault((at, "copy-to-borrow"), raw)
        if OUT_OF_A_CELL.search(code):
            found.setdefault((at, "copy-out-of-a-cell"), raw)
        if CONVERTED.search(code):
            reported.append((at + 1, "convert-by-reference", raw.strip()))
    out = [
        (at + 1, rule, text.strip())
        for (at, rule), text in sorted(found.items())
        if not explained(lines, at)
    ]
    return out, reported


def key_of(rel: str, rule: str, text: str) -> str:
    """A finding as the baseline keeps it: no line number, so a file edited
    above it does not move it."""
    return f"{rel}\t{rule}\t{' '.join(text.split())}"


MESSAGES = {
    "copy-per-item": "a copy an item: iterate references, move with into_iter, or say why with `// copy:`",
    "copy-to-iterate": "a copy only to loop over it: iterate `&y`, or say why with `// copy:`",
    "copy-to-borrow": "a copy only to borrow it: pass `&x`, or say why with `// copy:`",
    "copy-out-of-a-cell": "a copy to leave a borrow: share it behind an Rc, or say why with `// copy:`",
}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print the reported shapes")
    ap.add_argument("--write-baseline", action="store_true")
    args = ap.parse_args()

    found: list[tuple[str, int, str, str]] = []
    reports: list[tuple[str, int, str, str]] = []
    files = sources()
    for path in files:
        rel = path.relative_to(ROOT).as_posix()
        errors, reported = findings_of(path)
        found += [(rel, line, rule, text) for line, rule, text in errors]
        reports += [(rel, line, rule, text) for line, rule, text in reported]

    if args.write_baseline:
        keys = sorted(key_of(rel, rule, text) for rel, _, rule, text in found)
        BASELINE.write_text("\n".join(keys) + ("\n" if keys else ""))
        print(f"{len(keys)} findings written to {BASELINE.relative_to(ROOT)}")
        return 0

    kept = collections.Counter(BASELINE.read_text().splitlines()) if BASELINE.exists() else collections.Counter()
    fresh = []
    for rel, line, rule, text in found:
        key = key_of(rel, rule, text)
        if kept[key] > 0:
            kept[key] -= 1
        else:
            fresh.append((rel, line, rule))
    stale = sorted(key for key, left in kept.items() for _ in range(left))

    for rel, line, rule in fresh:
        print(f"ERROR  {rel}:{line}  [{rule}] {MESSAGES[rule]}")
    for key in stale:
        rel, rule, text = key.split("\t", 2)
        print(f"ERROR  {rel}  [baseline] no longer found, take its line out of scripts/copy_lints.baseline: {text[:80]}")
    if args.reports:
        for rel, line, rule, text in reports:
            print(f"report {rel}:{line}  [{rule}] a second copy if the value is not read again: {text[:80]}")
    held = sum(collections.Counter(BASELINE.read_text().splitlines()).values()) if BASELINE.exists() else 0
    print(
        f"\n{len(files)} files · {len(fresh) + len(stale)} errors · {held} in the baseline"
        f" · {len(reports)} reports (--reports)"
    )
    return 1 if (fresh or stale) and args.fail_on_error else 0


if __name__ == "__main__":
    sys.exit(main())
