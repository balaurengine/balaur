#!/usr/bin/env python3
"""House lints for Balaur.

These encode failure modes rather than taste. The rule for adding one: when the
same bad pattern shows up twice, it becomes a lint. This file is the memory.

Two severities:
  ERROR  fails CI. Mechanical, no judgement needed.
  REPORT prints but does not fail. Heuristics that will false-positive; a human
         or an AI pass confirms them. Failing the build on a heuristic teaches
         people to game the heuristic.

Usage:
  python3 scripts/house_lints.py                 # everything, exit 0 unless ERROR
  python3 scripts/house_lints.py --fail-on-error # same, explicit
  python3 scripts/house_lints.py --reports       # show REPORT findings too
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = Path(__file__).resolve().parent / "house_lints_baseline.txt"

SKIP_DIRS = {"target", ".git", "node_modules", "assets"}

MAX_COMMENT_BLOCK = 12   # consecutive comment lines
MAX_FN_LINES = 120
MAX_FILE_LINES = 1200

# Words too generic to count as "the comment said something new".
STOPWORDS = {
    "a", "an", "the", "of", "to", "for", "in", "on", "and", "or", "is", "are",
    "this", "that", "it", "its", "with", "from", "by", "as", "at", "be", "we",
    "returns", "return", "will", "when", "if", "then", "so", "not", "new",
}


@dataclass
class Finding:
    path: Path
    line: int
    rule: str
    message: str
    severity: str  # ERROR | REPORT


def rust_files() -> list[Path]:
    out = []
    for p in ROOT.rglob("*.rs"):
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        out.append(p)
    return sorted(out)


def stem(word: str) -> str:
    for suffix in ("ings", "ing", "ies", "ed", "es", "s"):
        if len(word) > len(suffix) + 2 and word.endswith(suffix):
            return word[: -len(suffix)]
    return word


def identifier_words(name: str) -> set[str]:
    parts = re.split(r"[_\s]+", re.sub(r"(?<!^)(?=[A-Z])", "_", name))
    return {stem(p.lower()) for p in parts if p}


def comment_words(text: str) -> set[str]:
    words = re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text.lower())
    return {stem(w) for w in words if w not in STOPWORDS and len(w) > 2}


def check_file(path: Path) -> list[Finding]:
    rel = path.relative_to(ROOT)
    findings: list[Finding] = []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()

    if len(lines) > MAX_FILE_LINES:
        findings.append(Finding(rel, len(lines), "file-too-long",
                                f"{len(lines)} lines, limit {MAX_FILE_LINES}", "ERROR"))

    in_test_mod = False
    test_brace_depth = None
    depth = 0
    fn_start = None
    fn_depth = None
    comment_run_start = None
    comment_run: list[str] = []
    comment_run_is_doc = False

    for i, raw in enumerate(lines, start=1):
        line = raw.strip()

        # --- comment runs -------------------------------------------------
        is_comment = line.startswith("//")
        # Doc comments (/// and //!) are API documentation and should be as long
        # as they need to be. The verbosity rule targets explanatory // comments.
        is_doc = line.startswith("///") or line.startswith("//!")
        if is_comment:
            if comment_run_start is None:
                comment_run_start = i
                comment_run_is_doc = is_doc
            comment_run.append(line.lstrip("/!").strip())
        else:
            if comment_run_start is not None:
                n = len(comment_run)
                if n > MAX_COMMENT_BLOCK and not comment_run_is_doc:
                    findings.append(Finding(rel, comment_run_start, "comment-too-long",
                                            f"{n} consecutive comment lines, limit {MAX_COMMENT_BLOCK}",
                                            "ERROR"))
                # comment that only restates the identifier below it
                m = re.match(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?"
                             r"(?:fn|struct|enum|trait|type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)", line)
                if m and comment_run:
                    ident = identifier_words(m.group(1))
                    body = comment_words(" ".join(comment_run))
                    if body and len(body - ident) == 0:
                        findings.append(Finding(
                            rel, comment_run_start, "comment-restates-name",
                            f"comment adds nothing over `{m.group(1)}`", "REPORT"))
                comment_run_start = None
                comment_run = []

        # --- test detection -----------------------------------------------
        if re.search(r"#\[cfg\(test\)\]", line) or "#[test]" in line:
            in_test_mod = True
            test_brace_depth = depth
        if "/tests/" in str(rel) or rel.name.startswith("test_"):
            in_test_mod = True

        # --- rules on code lines ------------------------------------------
        if not is_comment and line:
            if re.search(r"#\[allow\(", line) and "reason" not in line and "//" not in raw:
                findings.append(Finding(rel, i, "allow-without-reason",
                                        "#[allow(..)] needs a reason = \"..\" or a trailing comment",
                                        "ERROR"))

            if re.search(r"\b(TODO|FIXME|XXX)\b", raw) and not re.search(r"#\d+|issues?/\d+", raw):
                findings.append(Finding(rel, i, "todo-without-issue",
                                        "TODO/FIXME needs an issue reference", "ERROR"))

            if not in_test_mod and re.search(r"\.(unwrap|expect)\s*\(", line):
                # A descriptive .expect("..") IS the justification -- the rule
                # exists to stop silent panics, not to mandate a comment beside
                # a message that already explains itself. Bare .unwrap() and
                # filler messages still need one.
                msg = re.search(r"\.expect\s*\(\s*\"([^\"]*)\"", line)
                FILLER = {"failed", "error", "should work", "unwrap", "todo", "oops", "!"}
                self_explaining = bool(msg) and len(msg.group(1)) >= 12 \
                    and msg.group(1).strip().lower() not in FILLER
                commented = "//" in raw or (i > 1 and lines[i - 2].strip().startswith("//"))
                if not (self_explaining or commented):
                    findings.append(Finding(rel, i, "unjustified-unwrap",
                                            "unwrap/expect outside tests needs a justification comment "
                                            "or a descriptive expect message", "ERROR"))

            if re.search(r"\bfor\s+.*\bin\s+.*\b(HashMap|HashSet)\b", line) or \
               re.search(r"\b(HashMap|HashSet)\b.*\.(iter|keys|values)\(\)", line):
                findings.append(Finding(rel, i, "nondeterministic-iteration",
                                        "iterating a HashMap/HashSet leaks hash order into behaviour; "
                                        "use an ordered map or sort first", "ERROR"))

        # --- function length via brace depth ------------------------------
        if not is_comment:
            if fn_start is None and re.match(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?"
                                             r"(?:extern\s+\"[^\"]*\"\s+)?fn\s+", line):
                fn_start = i
                fn_depth = depth
            depth += raw.count("{") - raw.count("}")
            if fn_start is not None and fn_depth is not None and depth <= fn_depth and i > fn_start:
                length = i - fn_start + 1
                if length > MAX_FN_LINES:
                    findings.append(Finding(rel, fn_start, "fn-too-long",
                                            f"{length} lines, limit {MAX_FN_LINES}", "ERROR"))
                fn_start = None
                fn_depth = None
            if in_test_mod and test_brace_depth is not None and depth <= test_brace_depth:
                if "/tests/" not in str(rel):
                    in_test_mod = False
                    test_brace_depth = None

    return findings


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print REPORT findings")
    ap.add_argument("--update-baseline", action="store_true",
                    help="record current violations as the accepted debt")
    ap.add_argument("--debt", action="store_true", help="print the outstanding debt and exit")
    args = ap.parse_args()

    findings: list[Finding] = []
    files = rust_files()
    for path in files:
        findings.extend(check_file(path))

    errors = [f for f in findings if f.severity == "ERROR"]
    reports = [f for f in findings if f.severity == "REPORT"]

    # Ratchet: the baseline records violations that predate the lint. New ones
    # fail. Counts, not line numbers, so the baseline survives edits above them.
    counts: dict[str, int] = {}
    for f in errors:
        counts[f"{f.path}\t{f.rule}"] = counts.get(f"{f.path}\t{f.rule}", 0) + 1

    if args.update_baseline:
        BASELINE.write_text(
            "# Violations that predate the lint. Never add to this file by hand;\n"
            "# regenerate with: python3 scripts/house_lints.py --update-baseline\n"
            "# Every line here is debt. Deleting one is progress.\n"
            + "".join(f"{k}\t{v}\n" for k, v in sorted(counts.items())))
        print(f"baseline written: {sum(counts.values())} violations across {len(counts)} entries")
        return 0

    base: dict[str, int] = {}
    if BASELINE.exists():
        for line in BASELINE.read_text().splitlines():
            if line.startswith("#") or not line.strip():
                continue
            path, rule, n = line.split("\t")
            base[f"{path}\t{rule}"] = int(n)

    if args.debt:
        total = sum(base.values())
        print(f"outstanding debt: {total} violations across {len(base)} file/rule pairs")
        for k, v in sorted(base.items(), key=lambda kv: -kv[1]):
            p, r = k.split("\t")
            print(f"  {v:3d}  {r:28s} {p}")
        return 0

    new: list[Finding] = []
    seen: dict[str, int] = {}
    for f in errors:
        k = f"{f.path}\t{f.rule}"
        seen[k] = seen.get(k, 0) + 1
        if seen[k] > base.get(k, 0):
            new.append(f)

    for f in new:
        print(f"ERROR  {f.path}:{f.line}  [{f.rule}] {f.message}")
    if args.reports:
        for f in reports:
            print(f"report {f.path}:{f.line}  [{f.rule}] {f.message}")

    accepted = sum(base.values())
    print(f"\n{len(files)} files · {len(new)} new errors · {accepted} accepted "
          f"(debt: --debt) · {len(reports)} reports (--reports)")
    return 1 if (new and args.fail_on_error) else 0


if __name__ == "__main__":
    sys.exit(main())
