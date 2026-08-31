#!/usr/bin/env python3
"""Naming lints for the script API and the scene format.

`scripts/house_lints.py` globs `*.rs`, so it has never seen a single
script-API or scene-file name. This is that half: it boots the engine, asks it
what scripts can reach (`balaur api`, the same source `scripts/gen_docs.py`
reads), and checks the answer against docs/NAMING.md §3.

Reading a booted engine rather than the source is the point — derived
constants like `input.KEY_SPACE` exist only at registration time, and a name
scripts cannot actually see is not API. Three of the rules then go back to the
Rust source for evidence the JSON cannot carry: what a function returns, what
its declaration says about itself, and what a component's schema declares.

Two severities, as in house_lints.py:
  ERROR  fails CI. Mechanical, no judgement needed.
  REPORT prints but does not fail. Heuristics that will false-positive.

Usage:
  python3 scripts/api_lints.py                  # exit 0 unless ERROR
  python3 scripts/api_lints.py --fail-on-error  # same, explicit
  python3 scripts/api_lints.py --reports        # show REPORT findings too
  python3 scripts/api_lints.py --api-json f.json  # skip the build, read a dump
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP_DIRS = {"target", ".git", "node_modules", "assets"}

# N7. Each entry carries the reason, so the exemption stops being cited as
# precedent for the next `get_`. Both reasons are recorded in NAMING.md §5.
GETTER_ALLOW = {
    "node.get_node": "dropping the prefix gives `node:node(path)`",
    "scene.get_node": "dropping the prefix gives `scene:node(path)`",
    "node.get_component": "pairs with `component_names`; `node:component(k)` beside "
                          "`node:components()` is two functions one character apart with "
                          "unrelated return types",
}

# D2. A module is plural only when it is a keyed store of many things.
PLURAL_ALLOW = {"assets": "a keyed store of many things"}

# Singular words that happen to end in `s`. D2 lists every module besides
# `assets` as singular; these two are why the rule cannot just look at the
# last letter.
SINGULAR_IN_S = {
    "fs": "an initialism for the file system, not a plural",
    "physics": "a mass noun — there is no `physic`",
}

# D4: no abbreviations where users read them. Segment-exact, so `position`
# survives while `pos` does not, and `stroke` survives while `str` does not.
ABBREVIATIONS = {
    "str": "string", "cfg": "config", "buf": "buffer", "idx": "index", "pos": "position",
}
# The `h`+noun form, spelled out rather than pattern-matched so that
# `horizontal`, `has_component` and `height` are not casualties.
H_ABBREVIATIONS = {
    "hslider": "slider", "hbox": "row", "hsplit": "split", "hstack": "row",
    "hbar": "bar", "hline": "line", "hsep": "separator", "hpanel": "panel",
    "hscroll": "scroll", "hlayout": "layout",
}

# N6: the discriminant of a tagged union is `kind`, never one of these.
DISCRIMINANT_SYNONYMS = ("shape", "type", "mode", "variant")


@dataclass
class Finding:
    where: str
    rule: str
    message: str
    severity: str  # ERROR | REPORT


@dataclass
class Site:
    """Where a script name is declared in Rust, and what is said about it."""
    path: Path
    line: int
    comment: str
    body: str


def rust_files() -> list[Path]:
    out = []
    for dirpath, dirnames, filenames in os.walk(ROOT / "crates"):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS and not d.startswith("."))
        out.extend(Path(dirpath) / f for f in filenames if f.endswith(".rs"))
    return sorted(p for p in out if "/tests/" not in str(p) and "/benches/" not in str(p))


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


def block_at(text: str, start: int) -> str:
    """The braced block that follows `start`, so a rule can read a body."""
    depth, out, seen = 0, [], False
    for ch in text[start:start + 4000]:
        out.append(ch)
        if ch == "{":
            depth, seen = depth + 1, True
        elif ch == "}":
            depth -= 1
            if seen and depth <= 0:
                break
    return "".join(out)


def comment_above(lines: list[str], idx: int) -> str:
    out = []
    j = idx - 1
    while j >= 0 and lines[j].strip().startswith(("//", "#[")):
        out.append(lines[j].strip())
        j -= 1
    return "\n".join(reversed(out))


def declaration_sites() -> dict[str, list[Site]]:
    """Every script-visible name, mapped back to where Rust declares it.

    Two shapes: `m.function("name", |eng, ..| ..)` in a binding group, and the
    `NodeOp { name: "name", call: f }` tables, where the body lives in a free
    function one screen away.
    """
    sites: dict[str, list[Site]] = {}
    for path in rust_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.splitlines()
        fns = {m.group(1): m.start() for m in re.finditer(r"\nfn ([a-z_][a-z0-9_]*)\s*\(", text)}
        for m in re.finditer(r'\.function(?:_raw)?\(\s*"([a-z_][a-z0-9_]*)"', text):
            line = text.count("\n", 0, m.start()) + 1
            sites.setdefault(m.group(1), []).append(
                Site(path, line, comment_above(lines, line - 1), block_at(text, m.start())))
        for m in re.finditer(r'name:\s*"([a-z_][a-z0-9_]*)",\s*call:\s*([a-z_][a-z0-9_]*)', text):
            line = text.count("\n", 0, m.start()) + 1
            start = fns.get(m.group(2), m.start())
            sites.setdefault(m.group(1), []).append(
                Site(path, line, comment_above(lines, line - 1), block_at(text, start)))
    return sites


def boolean_vocabulary() -> tuple[set[str], set[str]]:
    """Every Rust fn declared `-> bool` and every field declared `bool`.

    This is how a script binding proves it returns a boolean: the closure body
    is `Ok(v)` where `v` came from one of these.
    """
    fns, fields = set(), set()
    for path in rust_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        fns.update(re.findall(r"fn\s+([a-z_][a-z0-9_]*)\s*\([^;{]*?\)\s*->\s*bool", text, re.S))
        fields.update(re.findall(r"^\s*(?:pub\s+)?([a-z_][a-z0-9_]*):\s*bool", text, re.M))
    return fns, fields


def boolean_shaped(body: str, bool_fns: set[str], bool_fields: set[str]) -> bool:
    if "-> bool" in body or "Value::Bool" in body:
        return True
    if re.search(r"\b(true|false)\b|[=!]=|\.contains\(|\.is_empty\(|\.any\(", body):
        return True
    if any(name in bool_fns for name in re.findall(r"\.([a-z_][a-z0-9_]*)\s*\(", body)):
        return True
    return any(name in bool_fields for name in re.findall(r"\.([a-z_][a-z0-9_]*)\s*[;,)\n]", body))


def check_functions(api: dict, sites: dict[str, list[Site]]) -> list[Finding]:
    """N7, N8, D4 over every function a script can call."""
    out: list[Finding] = []
    bool_fns, bool_fields = boolean_vocabulary()
    for module in api["modules"]:
        mod = module["name"]
        names = set(module["functions"])
        for fn in module["functions"]:
            where = f"{mod}.{fn}"
            at = sites.get(fn, [])
            loc = f"{at[0].path.relative_to(ROOT)}:{at[0].line}" if at else where
            if fn.startswith("get_") and where not in GETTER_ALLOW:
                out.append(Finding(loc, "getter-prefix",
                                   f"`{where}` is named for how it is implemented; a reader is "
                                   f"named for what it returns — `{mod}.{fn[4:]}` (N7)", "ERROR"))
            if fn.startswith("is_") and at and not any(
                    boolean_shaped(s.body, bool_fns, bool_fields) for s in at):
                out.append(Finding(loc, "is-prefix-nonboolean",
                                   f"`{where}`: `is_` marks a boolean reader, and nothing in the "
                                   "binding says this one returns a bool (N7)", "ERROR"))
            out.extend(abbreviation_findings(where, fn, loc))
            if fn.startswith("set_"):
                out.extend(setter_findings(mod, fn, names, at, loc))
    return out


def setter_findings(mod, fn, names, at, loc) -> list[Finding]:
    """N8: every setter has a reader, or says at its declaration why not."""
    read = fn[4:]
    if {read, f"is_{read}", f"get_{read}"} & names:
        return []
    if any(s.comment for s in at):
        return []
    return [Finding(loc, "setter-without-reader",
                    f"`{mod}.{fn}` has no `{read}` or `is_{read}` in its module and no "
                    "justification comment at its declaration — the value it writes is "
                    "already in the typemap (N8)", "REPORT")]


def abbreviation_findings(where, name, loc) -> list[Finding]:
    out = []
    for seg in name.lower().split("_"):
        spelled = ABBREVIATIONS.get(seg) or H_ABBREVIATIONS.get(seg)
        if spelled:
            out.append(Finding(loc, "abbreviation",
                               f"`{where}`: `{seg}` is read by people who did not write the "
                               f"engine — `{spelled}` (D4)", "ERROR"))
    return out


def check_modules(api: dict) -> list[Finding]:
    """D2: a module name is the noun for what the module owns."""
    out = []
    for module in api["modules"]:
        name = module["name"]
        for const in module["constants"]:
            out.extend(abbreviation_findings(f"{name}.{const['name']}", const["name"], name))
        if not name.endswith("s") or name in PLURAL_ALLOW or name in SINGULAR_IN_S:
            continue
        out.append(Finding(name, "module-plural",
                           f"`{name}` is plural: a module is plural only when it is a keyed "
                           "store of many things. Add the reason to PLURAL_ALLOW and to "
                           "NAMING.md D2, or make it singular", "ERROR"))
    return out


def property_types() -> list[str]:
    """The closed set, read from the const balaur_core makes public for it."""
    text = (ROOT / "crates" / "balaur_core" / "src" / "components.rs").read_text()
    m = re.search(r"PROPERTY_TYPES:\s*\[&str;\s*\d+\]\s*=\s*\[([^\]]*)\]", text)
    if not m:
        print("balaur_core::components::PROPERTY_TYPES not found", file=sys.stderr)
        raise SystemExit(1)
    return re.findall(r'"([a-z0-9_]+)"', m.group(1))


def check_schemas() -> list[Finding]:
    """N6: the scene-file vocabulary, over every registered schema.

    The fourth clause of N6 — every key a component's `get` emits is declared
    in its schema — is a Rust test rather than a lint (NAMING.md Table C says
    so): it needs the live registry, not a regex over a closure.
    """
    out, types = [], property_types()
    for path in rust_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        for m in re.finditer(r'parse_schema\(\s*"([a-z0-9_]+)"\s*,\s*r#"(.*?)"#', text, re.S):
            component, line = m.group(1), text.count("\n", 0, m.start()) + 1
            loc = f"{path.relative_to(ROOT)}:{line}"
            try:
                schema = tomllib.loads(m.group(2))
            except tomllib.TOMLDecodeError as e:
                out.append(Finding(loc, "schema-vocabulary",
                                   f"`{component}`: schema is not valid TOML: {e}", "ERROR"))
                continue
            for prop, spec in schema.items():
                out.extend(schema_findings(loc, component, prop, spec, types))
    return out


def schema_findings(loc, component, prop, spec, types) -> list[Finding]:
    out = []
    declared = spec.get("type") if isinstance(spec, dict) else None
    if declared not in types:
        out.append(Finding(loc, "schema-vocabulary",
                           f"`{component}.{prop}`: `type` is {declared!r}, not one of "
                           f"{', '.join(types)} — the meta key declaring a datatype is `type`, "
                           "and its values are a closed set (N6)", "ERROR"))
    if isinstance(spec, dict) and "kind" in spec:
        out.append(Finding(loc, "schema-vocabulary",
                           f"`{component}.{prop}`: `kind` is a property *name* — the one reserved "
                           "for a tagged union's discriminant — never a meta key (N6)", "ERROR"))
    if declared == "enum" and prop in DISCRIMINANT_SYNONYMS:
        out.append(Finding(loc, "schema-vocabulary",
                           f"`{component}.{prop}`: a tagged union's discriminant is always named "
                           "`kind` (N6)", "ERROR"))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print REPORT findings")
    ap.add_argument("--api-json", help="read this dump instead of booting the engine")
    args = ap.parse_args()

    api = script_api(args.api_json)
    findings = check_modules(api) + check_functions(api, declaration_sites()) + check_schemas()

    errors = [f for f in findings if f.severity == "ERROR"]
    reports = [f for f in findings if f.severity == "REPORT"]
    for f in errors:
        print(f"ERROR  {f.where}  [{f.rule}] {f.message}")
    if args.reports:
        for f in reports:
            print(f"report {f.where}  [{f.rule}] {f.message}")

    modules = api["modules"]
    functions = sum(len(m["functions"]) for m in modules)
    print(f"\n{len(modules)} modules · {functions} functions · {len(errors)} errors "
          f"· {len(reports)} reports (--reports)")
    return 1 if (errors and args.fail_on_error) else 0


if __name__ == "__main__":
    sys.exit(main())
