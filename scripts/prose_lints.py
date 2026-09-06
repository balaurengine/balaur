#!/usr/bin/env python3
"""Prose lints for the two hand-written documents that flow into the website.
The rule is the website's (balaur-website scripts/lint-prose.mjs): a sentence
is under 35 words, no filler, and an em dash is typography only at the lead of
a list item or a roadmap row, never a splice in prose.

Two severities, as in house_lints.py:
  ERROR  CHANGELOG.md. One line per feature; fails CI.
  REPORT docs/ROADMAP.md. A row is a comma list of what an item covers, so a
         long one is a judgement call; printed, never failing.

  python3 scripts/prose_lints.py                 # exit 0 unless ERROR
  python3 scripts/prose_lints.py --fail-on-error # same, explicit
  python3 scripts/prose_lints.py --reports       # show REPORT findings too
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ERRORS = ["CHANGELOG.md"]
REPORTS = ["docs/ROADMAP.md"]

MAX_SENTENCE = 35

# Throat-clearing; the fix is to delete it.
PHRASES = [
    r"\bhonest(ly)?\b", r"\bactually\b", r"\btruly\b", r"\bgenuinely\b",
    r"\bit(?:'s| is) worth\b", r"\bworth (?:noting|a look|mentioning)\b",
    r"\bthe question (?:was|is)\b", r"\blet's\b", r"\bto be (?:clear|fair)\b",
    r"\bsimply put\b", r"\bin order to\b", r"\bat its core\b",
    r"\bleverag(?:e|es|ed|ing)\b", r"\bdelv(?:e|es|ed|ing)\b", r"\bseamless(?:ly)?\b",
]

# The em dash that is typography: `- **Term** — text`, `| **Term** — text`.
LEAD_DASH = re.compile(r"^\s*(?:[-*]|\d+\.|\|)\s*(?:\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))\s*—\s")


@dataclass
class Finding:
    path: str
    line: int
    rule: str
    message: str


def prose(line: str) -> str:
    """One line of markdown as plain words; inline code is one word."""
    t = re.sub(r"^\s*(?:[-*]|\d+\.)\s+", "", line)
    t = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", t)
    t = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", t)
    t = re.sub(r"`[^`]*`", "X", t)
    t = re.sub(r"<[^>]+>", "", t)
    t = t.replace("**", "").replace("__", "").replace("*", "")
    return t.strip()


def sentences(text: str) -> list[str]:
    text = re.sub(r"\b(e\.g|i\.e|vs|etc|cf)\.", r"\1", text, flags=re.I)
    return [s.strip() for s in re.split(r"(?<=[.!?])\s+(?=[A-Za-z0-9\"'X\[(`])", text) if s.strip()]


def lint(path: Path) -> list[Finding]:
    out: list[Finding] = []
    rel = str(path.relative_to(ROOT))
    fence = False
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if raw.lstrip().startswith("```"):
            fence = not fence
            continue
        if fence or not raw.strip() or raw.lstrip().startswith(("#", "<!--")):
            continue
        # A table row: the first cell is the prose; the rest are tier and plan.
        cells = [c for c in raw.split("|")] if raw.lstrip().startswith("|") else [raw]
        text = prose(cells[1] if len(cells) > 2 else cells[0])
        if re.fullmatch(r"[-:\s]*", text):
            continue
        for s in sentences(text):
            words = len(s.split())
            if words > MAX_SENTENCE:
                out.append(Finding(rel, n, "sentence-length",
                                   f"{words}-word sentence; the limit is {MAX_SENTENCE}: \"{s[:60]}…\""))
        for p in PHRASES:
            m = re.search(p, text, re.I)
            if m:
                out.append(Finding(rel, n, "phrase", f"\"{m.group(0)}\": say the fact without it"))
        if "—" in raw and not LEAD_DASH.match(raw):
            out.append(Finding(rel, n, "em-dash", "em dash in prose; use a comma, a colon, a period or a list"))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print REPORT findings")
    args = ap.parse_args()
    errors = [f for name in ERRORS if (ROOT / name).exists() for f in lint(ROOT / name)]
    reports = [f for name in REPORTS if (ROOT / name).exists() for f in lint(ROOT / name)]
    for f in errors:
        print(f"ERROR  {f.path}:{f.line}  [{f.rule}] {f.message}")
    if args.reports:
        for f in reports:
            print(f"report {f.path}:{f.line}  [{f.rule}] {f.message}")
    print(f"\n{len(ERRORS) + len(REPORTS)} files · {len(errors)} errors · {len(reports)} reports")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
