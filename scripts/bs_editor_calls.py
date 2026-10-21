#!/usr/bin/env python3
"""Every engine call the editor makes, checked against Balaur Script's API.

The editor moves from Rune to Balaur Script (docs/PLAN-engine-in-bs.md). This
reads every `module::name` in `editor/**/*.rn`, keeps the ones that name an
engine module rather than one of the editor's own files, and checks each
against what a `.bs` script compiles against: the `bs` section of
`balaur api`, which the backend builds from the same registry the compiler
reads.

  ERROR  an engine function or constant a `.bs` script cannot name, a call
         passing a number of arguments its `.bs` signature does not take, or
         a record a plugin declared whose fields do not read.
  REPORT a call whose result is an `any`, so the port reads it dynamically.

Node methods (`node.get_node(..)`) are not checked: the receiver's type is not
in the text. Their types live in `crates/balaur_script_bs/src/node_ops.rs`.

Usage:
  python3 scripts/bs_editor_calls.py                   # exit 0 unless ERROR
  python3 scripts/bs_editor_calls.py --fail-on-error
  python3 scripts/bs_editor_calls.py --reports         # the `any` results too
  python3 scripts/bs_editor_calls.py --api-json f.json # skip the build
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EDITOR = ROOT / "editor"

# Not engine calls a `.bs` script makes, each with what takes its place.
NOT_API = {
    # Rune's own: the port writes these in the language.
    "script::require": "a `use` between `.bs` files",
    "script::attempt": "a function that returns its error as a value",
    "script::shared": "a function ref, or a method another node calls",
    # Tooling over Rune's `mod` graph and declarations, rebuilt for `.bs`
    # files when the panels that call them are ported (PLAN step 6).
    "script::api": "the Docs dock's reference, from `balaur api`",
    "script::exports": "a `.bs` script's fields, from its compiled `Meta`",
    "script::functions": "a `.bs` script's functions, from its compiled `Meta`",
    "script::find": "a text search over a file and the files it `use`s",
    "script::replace": "the same, rewritten",
}

PATH = re.compile(r"(?<![\w:.])([a-z_][a-z0-9_]*)((?:::[A-Za-z_][A-Za-z0-9_]*)+)")
USE = re.compile(r"\buse\s+(?:crate|super)::([a-z_][a-z0-9_]*)")
MOD = re.compile(r"^\s*(?:pub\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;", re.M)


@dataclass
class Finding:
    where: str
    rule: str
    message: str
    severity: str


def script_api(dump: str | None) -> dict:
    """What a booted engine says scripts can reach."""
    if dump:
        return json.loads(Path(dump).read_text())
    out = subprocess.run(
        ["cargo", "run", "-q", "-p", "balaur_cli", "--bin", "balaur", "--", "api"],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    if out.returncode != 0:
        print(f"`balaur api` failed ({out.returncode}):\n{out.stderr.rstrip()}", file=sys.stderr)
        raise SystemExit(1)
    return json.loads(out.stdout)


def code_only(src: str) -> str:
    """`src` with comments and string text blanked, lines and columns kept.

    A template string's `${..}` stays: it is code.
    """
    out, i, n = list(src), 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c, nxt = src[i], src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            end = src.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
        elif c == "/" and nxt == "*":
            end = src.find("*/", i + 2)
            end = n if end < 0 else end + 2
            blank(i, end)
            i = end
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, j)
            i = j + 1
        elif c == "'" and (i + 2 < n and (src[i + 2] == "'" or src[i + 1] == "\\")):
            j = src.find("'", i + 2 if src[i + 1] == "\\" else i + 1)
            j = n if j < 0 else j
            blank(i + 1, j)
            i = j + 1
        elif c == "`":
            j = i + 1
            while j < n and src[j] != "`":
                if src[j] == "\\":
                    blank(j, j + 2)
                    j += 2
                elif src.startswith("${", j):
                    depth, j = 1, j + 2
                    while j < n and depth:
                        depth += {"{": 1, "}": -1}.get(src[j], 0)
                        j += 1
                else:
                    blank(j, j + 1)
                    j += 1
            i = j + 1
        else:
            i += 1
    return "".join(out)


def arg_count(code: str, open_at: int) -> int:
    """How many arguments the call whose `(` is at `open_at` passes."""
    depth, i, count, arg = 0, open_at, 0, ""
    while i < len(code):
        c = code[i]
        if c in "([{":
            depth += 1
            if depth > 1:
                arg += c
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                return count + (1 if arg.strip() else 0)
            arg += c
        elif c == "," and depth == 1:
            count += 1
            arg = ""
        elif c == "|" and depth == 1 and arg.strip() in ("", "move", "async", "async move"):
            # A closure's parameters: their commas are not the call's.
            close = code.find("|", i + 1)
            arg += "|" + code[i + 1:close] + "|"
            i = close
        else:
            arg += c
        i += 1
    return count


def params(signature: str) -> int:
    """How many parameters `(a, b) -> r` takes."""
    depth, count, seen = 0, 0, False
    for c in signature:
        if c in "([":
            depth += 1
            if depth == 1:
                continue
        elif c in ")]":
            depth -= 1
            if depth == 0:
                break
        elif c == "," and depth == 1:
            count += 1
            continue
        if depth >= 1 and not c.isspace():
            seen = True
    return count + 1 if seen else 0


def own_modules() -> set[str]:
    """What `editor.rn` declares with `mod`: the editor's own files."""
    root = EDITOR / "scripts" / "editor.rn"
    return set(MOD.findall(root.read_text())) if root.exists() else set()


def scan(api: dict) -> tuple[list[Finding], Counter, Counter]:
    engine = {m["name"] for m in api["modules"]}
    bs = api["bs"]
    functions, constants = bs["functions"], bs["constants"]
    declared = own_modules()
    findings: list[Finding] = []
    calls: Counter = Counter()
    names: Counter = Counter()
    for path in sorted(EDITOR.rglob("*.rn")):
        rel = path.relative_to(ROOT)
        code = code_only(path.read_text())
        shadowed = set(USE.findall(code))
        if path.name == "editor.rn":
            shadowed |= declared
        for m in PATH.finditer(code):
            head, rest = m.group(1), m.group(2)
            segments = rest.split("::")[1:]
            if head not in engine or head in shadowed or len(segments) != 1:
                continue
            name = segments[0]
            key = f"{head}::{name}"
            line = code.count("\n", 0, m.start()) + 1
            where = f"{rel}:{line}"
            after = code[m.end():].lstrip()
            names[key] += 1
            if name[0].isupper():
                if key not in constants:
                    findings.append(Finding(where, "no-constant",
                                            f"`{key}` is no constant a `.bs` script can name",
                                            "ERROR"))
                continue
            if key in NOT_API:
                continue
            row = functions.get(key)
            if row is None:
                findings.append(Finding(where, "unreachable",
                                        f"`{key}` has no `.bs` signature", "ERROR"))
                continue
            if not after.startswith("("):
                continue
            calls[key] += 1
            given = arg_count(code, m.end() + (len(code[m.end():]) - len(after)))
            least, most = row["least"], params(row["signature"])
            if not least <= given <= most:
                takes = str(most) if least == most else f"{least} to {most}"
                findings.append(Finding(where, "arity",
                                        f"`{key}` takes {takes} argument(s) in `.bs`, "
                                        f"{given} given: {row['signature']}", "ERROR"))
            if row["signature"].endswith("-> any"):
                findings.append(Finding(where, "any-result",
                                        f"`{key}` gives an `any`: {row['signature']}", "REPORT"))
    return findings, calls, names


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print REPORT findings")
    ap.add_argument("--api-json", help="read this dump instead of booting the engine")
    args = ap.parse_args()

    api = script_api(args.api_json)
    if "bs" not in api:
        print("the dump has no `bs` section: build `balaur` with `bs`", file=sys.stderr)
        return 1
    findings, calls, names = scan(api)
    for record in api["bs"].get("unread", []):
        findings.append(Finding(record, "unread-record",
                                "a field's type is no `.bs` type, so nothing returns it",
                                "ERROR"))
    errors = [f for f in findings if f.severity == "ERROR"]
    reports = [f for f in findings if f.severity == "REPORT"]
    for f in errors:
        print(f"ERROR  {f.where}  [{f.rule}] {f.message}")
    if args.reports:
        for f in reports:
            print(f"report {f.where}  [{f.rule}] {f.message}")
    print(f"\n{len(names)} engine names · {sum(names.values())} references · "
          f"{sum(calls.values())} calls checked · {len(errors)} errors · "
          f"{len(reports)} reports (--reports)")
    return 1 if (errors and args.fail_on_error) else 0


if __name__ == "__main__":
    sys.exit(main())
