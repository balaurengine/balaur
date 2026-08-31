#!/usr/bin/env python3
"""House lints for Balaur.

These encode failure modes rather than taste. The rule for adding one: when the
same bad pattern shows up twice, it becomes a lint. This file is the memory.

Two severities:
  ERROR  fails CI. Mechanical, no judgement needed.
  REPORT prints but does not fail. Heuristics that will false-positive; a human
         or an AI pass confirms them. Failing the build on a heuristic teaches
         people to game the heuristic.

The naming rules (docs/NAMING.md §3, §6) live here too: they are Rust-side and
mechanical. Their script-API siblings are in scripts/api_lints.py, which reads
a booted engine instead of the source.

Usage:
  python3 scripts/house_lints.py                 # everything, exit 0 unless ERROR
  python3 scripts/house_lints.py --fail-on-error # same, explicit
  python3 scripts/house_lints.py --reports       # show REPORT findings too
"""
from __future__ import annotations

import argparse
import os
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

# Type suffixes a typemap entry may never take (NAMING.md N2). A denylist, not
# an allowlist: "no suffix" is a legal category, so `ClearColor` and
# `DebugLineBuffer` would both pass any permissive check.
RESOURCE_DENY_SUFFIXES = ("Manager", "Info", "Data", "Settings", "Server", "Enabled", "Handler")

# Every type currently inserted into the typemap, with its D3 category already
# chosen by a human. A type not on this list is a REPORT, not an error: the
# lint's job is to make the choice happen, not to make it.
KNOWN_RESOURCES = {
    "AnimationState", "AppIconConfig", "AssetState", "AssetTypeRegistry",
    "AudioState", "CameraConfig", "CameraConfig2d", "CameraInputConfig",
    "ClearColorConfig", "ComponentRegistry", "DebugLineBuffer", "DebugLineBuffer2d",
    "GamendSnapshot", "GamendState",
    "GridConfig", "InputSnapshot", "NetSnapshot", "NetState", "PhysicsState",
    "PhysicsState2d", "ProjectRoot",
    "RngState", "SceneKeyRegistry", "ScreenshotRequest", "ScriptArgs", "UiConfig",
    "UiState", "ViewportSnapshot", "ViewportSnapshot2d", "WidgetLayerConfig",
    "WindowedBackend",
    # Inserted as `manifest.clone()` (app.rs), so no regex will ever see it.
    "ProjectManifest",
}

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


def is_test_file(rel):
    """Tests and benchmarks: scaffolding, not shipped code."""
    p = str(rel)
    return "/tests/" in p or "/benches/" in p or rel.name.startswith("test_")


def rust_files() -> list[Path]:
    """Prunes as it walks rather than globbing and filtering: `target/` is
    enormous, and its contents come and go under a running cargo, which a
    post-hoc filter turns into a crash."""
    out = []
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS and not d.startswith("."))
        out.extend(Path(dirpath) / f for f in filenames if f.endswith(".rs"))
    return sorted(out)


@dataclass
class Context:
    """What the naming rules need to know about the whole tree at once.

    Gathered in one pass before any file is checked, because three of the rules
    are only decidable against a list that lives in other files: the glued
    `2d` spellings are legal exactly when they quote a user-facing name, and
    `Det` is legal exactly on the collections.rs aliases.
    """
    glued: set[str]         # component keys, script module names, dependency crates
    det_aliases: set[str]   # the sanctioned Det* type aliases
    plugin_crates: set[str] # crates with an `impl Plugin`, where install/register mean something


def scan_context(files: list[Path]) -> Context:
    glued, det_aliases, plugin_crates = set(), set(), set()
    for path in files:
        text = path.read_text(encoding="utf-8", errors="replace")
        for pat in (r'register_component\(\s*"([a-z0-9_]+)"', r'script_module\(\s*"([a-z0-9_]+)"'):
            glued.update(re.findall(pat, text, re.S))
        if path.name == "collections.rs":
            det_aliases.update(re.findall(r"type\s+(Det[A-Za-z0-9_]*)", text))
        if "impl Plugin for" in text:
            plugin_crates.add(crate_of(path))
    # A dependency's own spelling is not ours to fix: `rapier2d` is glued
    # because the crate is called that.
    for manifest in list(ROOT.glob("Cargo.toml")) + sorted((ROOT / "crates").glob("*/Cargo.toml")):
        for name in re.findall(r"^([A-Za-z0-9_-]+)\s*=", manifest.read_text(), re.M):
            glued.add(name.replace("-", "_"))
    return Context(glued, det_aliases, plugin_crates)


def crate_of(path: Path) -> str:
    rel = path.relative_to(ROOT).parts
    return rel[1] if len(rel) > 1 and rel[0] == "crates" else rel[0]


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


def identifiers(line: str) -> list[str]:
    """Identifiers in the code on this line, with strings and trailing comments
    dropped: the naming rules are about names, and a scene key or a project
    name quoted in a literal is neither ours to spell nor a declaration."""
    code = re.sub(r'"(?:\\.|[^"\\])*"', '""', line)
    return re.findall(r"[A-Za-z_][A-Za-z0-9_]*", code.split("//")[0])


def dimension_rules(rel, i, line, ctx) -> list[Finding]:
    """N3 and N4/N5: `Det` and the two spellings of the dimension."""
    out = []
    for ident in identifiers(line):
        if ident.startswith("Det") and re.match(r"Det[A-Z]", ident) and ident not in ctx.det_aliases:
            out.append(Finding(rel, i, "det-prefix-misuse",
                               f"`{ident}`: Det marks a fixed-iteration-order collection in "
                               "collections.rs and nothing else (N3)", "ERROR"))
        if re.fullmatch(r"[A-Z][A-Za-z0-9]*", ident):
            # SCREAMING_SNAKE has no lowercase to be consistent with, and never
            # matches the CamelCase test above (N4 exempts it explicitly).
            if "2D" in ident:
                out.append(Finding(rel, i, "dimension-casing",
                                   f"`{ident}`: the dimension is a lowercase `2d` (N4)", "ERROR"))
            elif "2d" in ident and not ident.endswith("2d"):
                out.append(Finding(rel, i, "dimension-casing",
                                   f"`{ident}`: `2d` goes at the end, so the name sorts next to "
                                   "its 3D twin (N4)", "ERROR"))
        elif re.fullmatch(r"[a-z0-9_]+", ident) and "2d" in ident:
            for seg in ident.split("_"):
                if "2d" in seg and seg != "2d" and seg not in ctx.glued:
                    out.append(Finding(rel, i, "dimension-snake",
                                       f"`{ident}`: `_2d` is its own word unless the segment "
                                       f"quotes a component key or module name; `{seg}` is "
                                       "neither (N5)", "ERROR"))
    return out


def signature(lines: list[str], idx: int) -> str:
    """The `fn` line joined with its continuations, up to the closing paren.

    Starts at the `fn` keyword, so the `(crate)` in `pub(crate) fn` is not
    mistaken for the parameter list.
    """
    text = " ".join(raw.strip() for raw in lines[idx:idx + 12])
    text = text[text.index("fn "):] if "fn " in text else text
    depth, out = 0, ""
    for ch in text:
        out += ch
        depth += (ch == "(") - (ch == ")")
        if depth == 0 and "(" in out:
            break
    return out


def first_param(sig: str) -> str:
    inner = sig[sig.index("(") + 1:] if "(" in sig else ""
    depth, out = 0, ""
    for ch in inner.replace("->", "  "):
        if ch in "(<[":
            depth += 1
        elif ch in ")>]":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            break
        out += ch
    return out.strip()


def noted_at_declaration(lines: list[str], idx: int, needle: str) -> bool:
    """NAMING.md Table A's escape hatch: "note that at the declaration".

    `install_engine_api(eng: &Engine)` and `install_ui_api(app: &mut App)` are
    both sanctioned departures, and both say so in a doc line that names the
    type they take instead. Anything else with that shape is an accident.
    """
    text = []
    j = idx - 1
    while j >= 0 and (lines[j].strip().startswith(("//", "#[")) or not lines[j].strip()):
        text.append(lines[j])
        j -= 1
    joined = "\n".join(text)
    return "Takes" in joined and needle in joined


def seam_rules(rel, i, line, lines, ctx, crate, impl_name) -> list[Finding]:
    """N10: the verb is bound to the parameter type."""
    m = re.match(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+"
                 r"(install|register)(_[a-z0-9_]+)?\s*\(", line)
    if not m:
        return []
    verb, suffix = m.group(1), m.group(2)
    if suffix is None:
        if crate in ctx.plugin_crates:
            return [Finding(rel, i, "install-verb",
                            f"bare `fn {verb}(` in a crate with a plugin seam: say what it "
                            f"fills — `{verb}_<thing>` — or use a different verb (N10)", "ERROR")]
        return []
    want = "&mut dyn Bindings" if verb == "install" else "&mut App"
    param = first_param(signature(lines, i - 1))
    if want in param:
        return []
    # An inherent method on App is `&mut App` under another spelling.
    if verb == "register" and param.startswith("&mut self") and impl_name == "App":
        return []
    types = re.findall(r"[A-Z][A-Za-z0-9]*", param)
    if noted_at_declaration(lines, i - 1, types[-1] if types else param):
        return []
    job = ("declares script functions" if verb == "install"
           else "registers scene components")
    return [Finding(rel, i, "install-verb",
                    f"`fn {verb}{suffix}` takes `{param or 'nothing'}`, not `{want}`: "
                    f"{verb}_* {job} (N10). A sanctioned departure says so in a doc line "
                    "naming what it takes instead.", "ERROR")]


def item_rules(rel, i, line) -> list[Finding]:
    """N12, N13, N15: suffixes on items, and the two frame vocabularies."""
    out = []
    if re.search(r"\bengine\s*:\s*&(?:mut\s+)?Engine\b", line):
        out.append(Finding(rel, i, "engine-param-name",
                           "one local name per concept: `eng: &Engine` (N13)", "ERROR"))
    m = re.search(r"\b(struct|enum)\s+([A-Za-z0-9_]*Fn)\b", line)
    if m:
        out.append(Finding(rel, i, "fn-suffix-on-struct",
                           f"`{m.group(2)}`: the Fn suffix is for aliases of a boxed or pointer "
                           "callable, never a struct or enum (N12)", "ERROR"))
    m = re.search(r"\bpub\s+(struct|enum)\s+([A-Za-z0-9_]*Inner)\b", line)
    if m:
        out.append(Finding(rel, i, "pub-inner",
                           f"`{m.group(2)}`: no `pub` item is named *Inner — it publishes what "
                           "the outer type's accessors exist to mediate (N12)", "ERROR"))
    m = re.search(r"add_system\([^,]*,\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)", line)
    if m and not m.group(1).split("::")[-1].endswith("_system"):
        out.append(Finding(rel, i, "system-verb",
                           f"`{m.group(1)}` is added as a system, so it is named `*_system`; a "
                           "backend loop step takes an apply_/publish_/flush_/pump_/sync_ verb "
                           "instead (N15)", "ERROR"))
    return out


def resource_rules(rel, i, line) -> list[Finding]:
    """N2: what a typemap entry may be called."""
    m = re.search(r"insert_resource\(\s*([A-Za-z_][A-Za-z0-9_:]*)", line)
    if not m:
        return []
    # The last capitalised path segment: `balaur::render::ScreenshotRequest`
    # and `RngState::default` both name a type; `manifest.clone()` does not,
    # and that site is real (app.rs), so no match is not a failure.
    types = [s for s in m.group(1).split("::") if s[:1].isupper()]
    if not types:
        return []
    name = types[-1]
    for bad in RESOURCE_DENY_SUFFIXES:
        if name.endswith(bad):
            return [Finding(rel, i, "resource-suffix",
                            f"`{name}`: `{bad}` is not one of the six suffixes a typemap entry "
                            "may take — Config, State, Snapshot, Buffer, Request, Registry — "
                            "or none (N2)", "ERROR")]
    if name not in KNOWN_RESOURCES:
        return [Finding(rel, i, "new-resource-type",
                        f"`{name}` is a new typemap entry: pick its D3 category and record it in "
                        "KNOWN_RESOURCES (N2)", "REPORT")]
    return []


def registration_documented(lines: list[str], idx: int) -> bool:
    """A doc comment above the `register_component(` call, or above its fn."""
    j = idx - 1
    while j >= 0:
        s = lines[j].strip()
        if s.startswith("//"):
            return True
        if s and not re.match(r"(?:pub(?:\([^)]*\))?\s+)?fn\s|^#\[|^\{$", s):
            return False
        j -= 1
    return False


def check_file(path: Path, ctx: Context) -> list[Finding]:
    rel = path.relative_to(ROOT)
    crate = crate_of(path)
    findings: list[Finding] = []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    impl_name = ""

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

        if re.search(r"#\[cfg\(test\)\]", line) or "#[test]" in line:
            in_test_mod = True
            test_brace_depth = depth
        if is_test_file(rel):
            in_test_mod = True

        if not is_comment and line:
            if re.search(r"#\[allow\(", line) and "reason" not in line:
                # A comment on the line above counts, same as for unwrap.
                commented = "//" in raw or (i > 1 and lines[i - 2].strip().startswith("//"))
                if not commented:
                    findings.append(Finding(rel, i, "allow-without-reason",
                                            "#[allow(..)] needs a reason = \"..\", a trailing "
                                            "comment, or a comment on the line above", "ERROR"))

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

            # We emit structured events, not strings: `log` records carry no
            # fields, so the editor's Output dock cannot filter them and an
            # observability test cannot assert on them. tracing-log bridges
            # dependencies that still use `log`; our own code must not.
            if re.search(r"\blog::(info|warn|error|debug|trace)!", line):
                findings.append(Finding(rel, i, "log-instead-of-tracing",
                                        "use tracing::* rather than log::*", "ERROR"))

            if re.search(r"\bfor\s+.*\bin\s+.*\b(HashMap|HashSet)\b", line) or \
               re.search(r"\b(HashMap|HashSet)\b.*\.(iter|keys|values)\(\)", line):
                findings.append(Finding(rel, i, "nondeterministic-iteration",
                                        "iterating a HashMap/HashSet leaks hash order into behaviour; "
                                        "use an ordered map or sort first", "ERROR"))

            m = re.match(r"impl(?:<[^>]*>)?\s+(?:[A-Za-z0-9_:<>, ]+\s+for\s+)?([A-Za-z_][A-Za-z0-9_]*)", line)
            if m:
                impl_name = m.group(1)
            findings.extend(dimension_rules(rel, i, line, ctx))
            findings.extend(seam_rules(rel, i, line, lines, ctx, crate, impl_name))
            findings.extend(item_rules(rel, i, line))
            # Tests build scratch types by the dozen and insert them to prove
            # the typemap works; the vocabulary rules are about shipped names.
            if not is_test_file(rel):
                findings.extend(resource_rules(rel, i, line))
                if "register_component(" in line and not re.search(r"fn\s+register_component", line):
                    if not registration_documented(lines, i - 1):
                        findings.append(Finding(
                            rel, i, "component-registration-doc",
                            "a registered component key needs a doc comment saying what state it "
                            "writes — the mapping from key to storage is neither one-to-one nor "
                            "total (N16)", "REPORT"))

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
                if not is_test_file(rel):
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
    ctx = scan_context(files)
    for path in files:
        findings.extend(check_file(path, ctx))

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
