#!/usr/bin/env python3
"""House lints for Balaur.

These encode failure modes rather than taste. The rule for adding one: when the
same bad pattern shows up twice, it becomes a lint. This file is the memory.

Two severities:
  ERROR  fails CI. Mechanical, no judgement needed.
  REPORT prints but does not fail. Heuristics that will false-positive; a human
         or an AI pass confirms them. Failing the build on a heuristic teaches
         people to game the heuristic.

The naming rules (docs/NAMING.md) live here too: they are Rust-side and
mechanical. Their script-API siblings are in scripts/api_lints.py, which reads
a booted engine instead of the source.

Usage:
  python3 scripts/house_lints.py                 # everything, exit 0 unless ERROR
  python3 scripts/house_lints.py --fail-on-error # same, explicit
  python3 scripts/house_lints.py --reports       # show REPORT findings too
  python3 scripts/house_lints.py --stats FILE    # append function and file sizes to FILE
"""
from __future__ import annotations

import argparse
import concurrent.futures
import os
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

SKIP_DIRS = {"target", ".git", "node_modules", "assets"}

MAX_COMMENT_BLOCK = 12   # consecutive comment lines
MAX_FN_LINES = 120
MAX_FILE_LINES = 1200
MAX_FN_BRANCHES = 20

# The float methods that differ across platform libms (docs/DETERMINISM.md).
# `sqrt`, `abs`, `floor`, `ceil`, `round`, `trunc`, `mul_add`, `to_radians`
# and `to_degrees` are IEEE-exact, and deliberately absent.
_INEXACT_FLOAT = (r"sin|cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|asinh|acosh"
                  r"|atanh|exp|exp2|exp_m1|ln|ln_1p|log|log2|log10|powf|cbrt|hypot")
# Both spellings: `x.sin()` and `f32::sin(x)` are the same call.
PLATFORM_FLOAT_RS = re.compile(rf"(?:\.|\bf(?:32|64)::)(?:{_INEXACT_FLOAT})\(")
# glam's own methods, which the workspace builds with glam's `libm` feature.
GLAM_BOUND = {"crates/balaur_script_rune/src/value/glam_api.rs"}

# egui's painter, reached from the widget layer: `widget/paint.rs` is the one
# file that holds it, and `lib.rs` the pass that owns the context and its font
# atlas.
EGUI_PAINT = re.compile(r"\.painter\(\)|\blayer_painter\s*\(|\begui::(?:Shape|epaint|Painter)\b|\bepaint::|\bShapeIdx\b")
EGUI_PAINT_SCOPE = "crates/balaur_ui/src/"
EGUI_PAINT_HOLDERS = {"crates/balaur_ui/src/widget/paint.rs", "crates/balaur_ui/src/lib.rs"}

# A literal key is a second spelling of what the crate's vocabulary already
# names (NAMING.md N17), and the reader takes the default instead of saying so.
VOCABULARY_LITERAL = re.compile(
    r'\bprop_(?:str|f32|f64|bool|i64|vec2|vec3)\([A-Za-z_][A-Za-z0-9_]*,\s*"'
    r'|\b(?:params|opts|table)\.get\("'
)

# A file written over where it stands, which a crash mid-write cuts short.
IN_PLACE_WRITE = re.compile(
    r'(?<![\w.])fs::(?:write|copy)\s*\(|\bFile::create\s*\(|\.write_in_place\s*\('
    r'|\.truncate\(\s*true\s*\)'
)
# `use std::fs::write;` or `use std::fs::{copy, write}`: the names a file
# then calls bare, where `fs::write(` is out of sight.
BARE_FS_IMPORT = re.compile(r'use\s+std::fs::(\{[^}]*\}|(?:write|copy)(?:\s+as\s+\w+)?\s*;)')
BARE_FS_NAME = re.compile(r'\b(?:write|copy)\b(?:\s+as\s+(\w+))?')
BARE_FS_CALL = re.compile(r'(?<![\w.:])(?<!fn )(\w+)\s*\(')
# An `OpenOptions` chain or a variable it is built in: refused unless the
# chain reaches its `.open(` appending, making the file new, or not writing.
OPEN_OPTIONS = re.compile(r'\bOpenOptions::new\s*\(\)|\bFile::options\s*\(\)')


def opens_in_place(lines, i) -> bool:
    """Whether the `OpenOptions` chain starting at line `i` may write over a
    file where it stands: it never reaches `.open(` in one chain (built in a
    variable), or reaches it writing, neither appending nor making it new."""
    chain = ""
    for raw in lines[i - 1:i + 11]:
        code = raw.split("//")[0]
        chain += code
        if ".open(" in code:
            break
        if ";" in code:
            return True
    return (".write(true)" in chain and ".append(true)" not in chain
            and ".create_new(true)" not in chain)


# A file moved over its target by hand, or a temporary named by hand.
HAND_WHOLE_MOVE = re.compile(r'(?<![\w.])fs::rename\s*\(|\.persist(?:_noclobber)?\s*\(')
HAND_PARTIAL_EXTENSION = re.compile(r'with_extension\(\s*"(?:part(?:ial)?|tmp|temp)"\s*\)')
STRING_LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"')
PARTIAL_IN_STRING = re.compile(r'\.part(?:ial)?(?:[.{]|$)')
WHOLE_OWNER = "crates/balaur_core/src/whole.rs"
FILES_OWNER = "crates/balaur_core/src/files.rs"


def scaffolding(rel) -> bool:
    """A build script, or the crates that time and test the engine: what they
    write a run makes again."""
    p = rel.as_posix()
    return rel.name == "build.rs" or p.startswith(("crates/balaur_bench/", "crates/balaur_testkit/"))


def hand_made_partial(line: str) -> bool:
    """Whether `line` writes a file whole by hand, or names its temporary."""
    if HAND_WHOLE_MOVE.search(line) or HAND_PARTIAL_EXTENSION.search(line):
        return True
    return any(PARTIAL_IN_STRING.search(lit[1:-1]) for lit in STRING_LITERAL.findall(line))

# A call that can re-enter the engine and ask for the resource whose `RefCell`
# the caller still holds; the panic that follows names neither side.
REENTRANT_CALL = re.compile(
    r"\b(?:call_on|call_all|call_async|invoke|hot_reload)\s*\("
    r"|\bhost\.(?:update|fixed_update|attach|call)\s*\("
    r"|\(\s*def\.(?:apply|get|remove)\s*\)\s*\("
    r"|\bcomponents::(?:add|patch|remove|remove_present)\s*\("
    r"|\binstantiate_scene\s*\("
)
# A thread woken on a timer to look, rather than by what it waits for:
# AGENTS.md "Threads". A frame cap or a real deadline says why on its line.
TIMED_WAIT = re.compile(
    r"\bthread::sleep\s*\(|\.set_(?:read|write)_timeout\s*\(|\btokio::time::(?:sleep|interval)\s*\("
    r"|\.(?:recv|wait|park)_timeout\s*\(|\bthread::park_timeout\s*\("
)

# A node's pose or appearance written in place. No change tick hears that
# write, so the scene sync never places the node again and it stands still on
# screen (ARCHITECTURE.md, Frame schedule: `SceneSync` places only what changed).
PLACEMENT_BORROW = re.compile(
    r"&(?:'\w+\s+)?mut\s+(?:\w+::)*(Transform|GlobalTransform|Appearance|GlobalAppearance)\b"
)
# What answers each in a file: its stamp, or a call that places every node
# anyway, a whole walk or a restore. A world pose is the sync's alone.
PLACEMENT_ANSWERS = {
    "Transform": ("transform::written(", "propagate_transforms", "ticks::restored("),
    "Appearance": ("appearance_written(", "propagate_transforms", "ticks::restored("),
    "GlobalTransform": (),
    "GlobalAppearance": (),
}
PLACEMENT_STAMP = {
    "Transform": "balaur_core::transform::written",
    "Appearance": "balaur_core::scene::appearance_written",
}
# The scene sync itself, which writes every world pose.
PLACEMENT_OWNER = "crates/balaur_core/src/scene/place.rs"

# `let x = <something>.borrow_mut();` — the exclusive binding, not a temporary,
# which is dropped at the end of its statement and cannot span a call.
BORROW_BINDING = re.compile(
    r"^\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*.*\.borrow_mut\(\)\s*;"
)

# Type suffixes a typemap entry may never take (CONTRIBUTING.md N2). A denylist, not
# an allowlist: "no suffix" is a legal category, so `ClearColor` and
# `DebugLineBuffer` would both pass any permissive check.
RESOURCE_DENY_SUFFIXES = ("Manager", "Info", "Data", "Settings", "Server", "Enabled", "Handler")

# Every type currently inserted into the typemap, with its D3 category already
# chosen by a human. A type not on this list is a REPORT, not an error: the
# lint's job is to make the choice happen, not to make it.
KNOWN_RESOURCES = {
    "AnimationState", "AppIconConfig", "AssetState", "AssetTypeRegistry",
    "AudioState", "CameraConfig3d", "CameraConfig2d", "CameraInputConfig",
    "ClearColorConfig", "ComponentRegistry", "DebugLineBuffer3d", "DebugLineBuffer2d",
    "GamendSnapshot", "GamendState",
    "GridConfig", "HttpSnapshot", "ImportState", "HttpState", "InputSnapshot", "PhysicsState3d", "WebsocketSnapshot", "WebsocketState",
    "PhysicsState2d", "PostConfig", "ProjectRoot",
    "RngState", "SceneKeyRegistry", "ScreenshotRequest", "ScriptArgs", "TextGeometry", "UiConfig",
    "UiState", "ViewportSnapshot3d", "ViewportSnapshot2d", "WidgetInputBuffer",
    "WidgetInputSnapshot", "WidgetLayerConfig",
    "WindowedBackend",
    # Inserted as `manifest.clone()` (app.rs), so no regex will ever see it.
    "ProjectManifest",
}


# A string or char literal, so a brace inside one (`self.want("{")`) is not
# taken for a block when a function's length is measured.
LITERAL = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\'')

RAW_OPEN = re.compile(r'(?<![\w\\"\'])r(#*)"')


def outside_raw(raw: str, closing: str | None) -> tuple[str, str | None]:
    """The line with its raw-string text taken out, and the closing a raw
    string still open at the line's end waits for. A test's script source in
    `r#"..."#` is not Rust, so its `if`s and braces are not the function's."""
    out = []
    at = 0
    while True:
        if closing is not None:
            end = raw.find(closing, at)
            if end < 0:
                return "".join(out), closing
            out.append('""')
            at = end + len(closing)
            closing = None
        opened = RAW_OPEN.search(raw, at)
        if not opened:
            out.append(raw[at:])
            return "".join(out), None
        out.append(raw[at:opened.start()])
        closing = '"' + opened.group(1)
        at = opened.end()


RS_FN = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]*\"\s+)?fn\s+")
BS_FN = re.compile(r"(?:(?:pub|internal)\s+)?(?:async\s+)?fn\s+\w")
# The branches a person wrote. A `match` counts once, as clippy counts it; clippy's
# own `cognitive_complexity` also counts what `assert_eq!` and the like expand to.
BRANCH = re.compile(r"\b(?:if|match|for|while)\b|^\s*let\b(?!.*\b(?:if|match)\b).*\belse\s*\{")


@dataclass
class Measured:
    path: Path
    line: int
    lines: int
    branches: int


# Every function and file the size rules measured, for `--stats`.
MEASURED_FNS: list[Measured] = []
MEASURED_FILES: list[tuple[Path, int]] = []


@dataclass
class Finding:
    path: Path
    line: int
    rule: str
    message: str
    severity: str  # ERROR | REPORT


@dataclass
class OpenFn:
    start: int
    depth: int
    branches: int = 0
    opened: bool = False

    def feed(self, code: str) -> None:
        self.branches += len(BRANCH.findall(code.split("//")[0]))
        self.opened = self.opened or "{" in code

    def ends(self, depth: int, code: str) -> bool:
        """A multi-line signature sits at the function's own depth before its body opens."""
        return depth <= self.depth and (self.opened or code.rstrip().endswith(";"))

    def findings(self, rel: Path, end: int) -> list[Finding]:
        if not self.opened:
            return []
        out = []
        length = end - self.start + 1
        MEASURED_FNS.append(Measured(rel, self.start, length, self.branches))
        if length > MAX_FN_LINES:
            out.append(Finding(rel, self.start, "fn-too-long",
                               f"{length} lines, limit {MAX_FN_LINES}", "ERROR"))
        if self.branches > MAX_FN_BRANCHES:
            out.append(Finding(rel, self.start, "fn-too-many-branches",
                               f"{self.branches} branches, limit {MAX_FN_BRANCHES}; split it, "
                               "or make an if-chain a match or a table", "ERROR"))
        return out


def is_test_file(rel):
    """Tests and benchmarks: scaffolding, not shipped code."""
    p = str(rel)
    return "/tests/" in p or "/benches/" in p or rel.name.startswith("test_")


def source_files(suffix: str) -> list[Path]:
    """Prunes as it walks rather than globbing and filtering: `target/` is
    enormous, and its contents come and go under a running cargo, which a
    post-hoc filter turns into a crash."""
    out = []
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS and not d.startswith("."))
        out.extend(Path(dirpath) / f for f in filenames if f.endswith(suffix))
    return sorted(out)


# A generator's output answers to its generator: `scripts/gen_steam.py`
# writes Valve's whole API, a file and a function as long as Valve's.
GENERATED = re.compile(r"@generated|generated by|do not edit", re.I)


def rust_files() -> list[Path]:
    return [p for p in source_files(".rs")
            if not GENERATED.search(p.read_text(encoding="utf-8", errors="replace")[:400])]


def check_script(path: Path) -> list[Finding]:
    """The size rules, for Balaur Script: no clippy reads it."""
    rel = path.relative_to(ROOT)
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    MEASURED_FILES.append((rel, len(lines)))
    findings: list[Finding] = []
    if len(lines) > MAX_FILE_LINES:
        findings.append(Finding(rel, len(lines), "file-too-long",
                                f"{len(lines)} lines, limit {MAX_FILE_LINES}", "ERROR"))
    depth = 0
    open_fn: OpenFn | None = None
    for i, raw in enumerate(lines, start=1):
        line = raw.strip()
        if line.startswith("//"):
            continue
        code = LITERAL.sub("", raw)
        if open_fn is None and BS_FN.match(line):
            open_fn = OpenFn(i, depth)
        if open_fn:
            open_fn.feed(code)
        depth += code.count("{") - code.count("}")
        if open_fn and open_fn.ends(depth, code):
            findings.extend(open_fn.findings(rel, i))
            open_fn = None
    return findings


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
    foreign: set[str]       # CamelCase names a dependency declares, named through its path


def scan_context(files: list[Path]) -> Context:
    glued, det_aliases, plugin_crates = set(), set(), set()
    for path in files:
        text = path.read_text(encoding="utf-8", errors="replace")
        # `host.module("...")` is how core registers its own script modules,
        # and an asset type's name is user-facing the same way a key is.
        for pat in (r'register_component\(\s*"([a-z0-9_]+)"', r'script_module\(\s*"([a-z0-9_]+)"',
                    r'register_asset_type\(\s*"([a-z0-9_]+)"',
                    r'host\.module\(\s*"([a-z0-9_]+)"',
                    # A derived component names itself on its struct.
                    r'#\[component\(\s*name\s*=\s*"([a-z0-9_]+)"'):
            glued.update(re.findall(pat, text, re.S))
        # A key named through a constant rather than at the call site. Only
        # the value is taken, and only where a plugin declares its vocabulary.
        if path.name == "vocabulary.rs" or any(
            marker in text
            for marker in ("register_component(", "script_module(", "register_asset_type(")
        ):
            glued.update(re.findall(r'const\s+[A-Z0-9_]+:\s*&str\s*=\s*"([a-z][a-z0-9_]*)"', text))
        if path.name == "collections.rs":
            det_aliases.update(re.findall(r"type\s+(Det[A-Za-z0-9_]*)", text))
        if "impl Plugin for" in text:
            plugin_crates.add(crate_of(path))
    # A dependency's own spelling is not ours to fix: `rapier2d` is glued
    # because the crate is called that.
    dependencies = set()
    for manifest in list(ROOT.glob("Cargo.toml")) + sorted((ROOT / "crates").glob("*/Cargo.toml")):
        for name in re.findall(r"^([A-Za-z0-9_-]+)\s*=", manifest.read_text(), re.M):
            glued.add(name.replace("-", "_"))
            if not name.startswith("balaur"):
                dependencies.add(name.replace("-", "_"))
    # The same holds for a type: `FirstPersonCamera3dStereo` is kiss3d's name,
    # taken wherever a file reaches it through the crate's own path.
    foreign = set()
    crates = "|".join(sorted(dependencies))
    for path in files:
        text = path.read_text(encoding="utf-8", errors="replace")
        for listed in re.findall(rf"\b(?:{crates})::(?:\w+::)*\{{([^}}]*)\}}", text):
            foreign.update(re.findall(r"\b[A-Z][A-Za-z0-9]*", listed))
        foreign.update(re.findall(rf"\b(?:{crates})::(?:\w+::)*([A-Z][A-Za-z0-9]*)", text))
    return Context(glued, det_aliases, plugin_crates, foreign)


def crate_of(path: Path) -> str:
    rel = path.relative_to(ROOT).parts
    return rel[1] if len(rel) > 1 and rel[0] == "crates" else rel[0]


def identifiers(line: str) -> list[str]:
    """Identifiers in the code on this line, with strings and trailing comments
    dropped: the naming rules are about names, and a scene key or a project
    name quoted in a literal is neither ours to spell nor a declaration."""
    code = re.sub(r'"(?:\\.|[^"\\])*"', '""', line)
    return re.findall(r"\b[A-Za-z_][A-Za-z0-9_]*", code.split("//")[0])


def dimension_rules(rel, i, line, ctx) -> list[Finding]:
    """N3 and N4/N5: `Det` and the two spellings of the dimension."""
    out = []
    for ident in identifiers(line):
        if ident.startswith("Det") and re.match(r"Det[A-Z]", ident) and ident not in ctx.det_aliases:
            out.append(Finding(rel, i, "det-prefix-misuse",
                               f"`{ident}`: Det marks a fixed-iteration-order collection in "
                               "collections.rs and nothing else (N3)", "ERROR"))
        if re.fullmatch(r"[A-Z][A-Za-z0-9]*", ident) and ident not in ctx.foreign:
            # SCREAMING_SNAKE has no lowercase to be consistent with, and never
            # matches the CamelCase test above (N4 exempts it explicitly).
            for dim in ("2d", "3d"):
                if dim.upper() in ident:
                    out.append(Finding(rel, i, "dimension-casing",
                                       f"`{ident}`: the dimension is a lowercase `{dim}` (N4)", "ERROR"))
                elif dim in ident and not ident.endswith(dim):
                    out.append(Finding(rel, i, "dimension-casing",
                                       f"`{ident}`: `{dim}` goes at the end, so the name sorts next "
                                       "to its twin (N4)", "ERROR"))
        elif re.fullmatch(r"[a-z0-9_]+", ident) and ("2d" in ident or "3d" in ident):
            for seg in ident.split("_"):
                if re.search(r"[23]d", seg) and seg not in ("2d", "3d") and seg not in ctx.glued:
                    out.append(Finding(rel, i, "dimension-snake",
                                       f"`{ident}`: `_2d`/`_3d` is its own word unless the segment "
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
    """N10's escape hatch: a departure noted at the declaration.

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


def channel_justified(lines: list[str], idx: int) -> bool:
    """A comment above the channel, or above the run of them it belongs to."""
    j = idx - 2
    while j >= 0 and re.search(r"(?:sync_)?channel\s*\(", lines[j]):
        j -= 1
    return j >= 0 and lines[j].strip().startswith("//")


def attribute(lines, i, limit=8) -> str:
    """The whole attribute starting at line `i`, which rustfmt may have wrapped."""
    text = ""
    for line in lines[i - 1:i - 1 + limit]:
        text += line
        if ")]" in line:
            break
    return text


# The kernel and the crates split from it: their
# registrations are an app's built-in steps, run before any plugin.
BUILTIN_CRATES = {"balaur_core", "balaur_geometry", "balaur_asset", "balaur_net",
                  "balaur_project", "balaur_debug"}


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
    want = "&mut Module" if verb == "install" else "&mut Registry"
    param = first_param(signature(lines, i - 1))
    if want in param:
        return []
    # `Registry` lives in a crate that depends on core, so core's own
    # registrations, and the built-in steps of the crates split from it, take
    # the `App` they are building.
    if verb == "register" and crate in BUILTIN_CRATES and "&mut App" in param:
        return []
    # An inherent method on either is that type under another spelling.
    if verb == "register" and param.startswith("&mut self") and impl_name in ("App", "Registry"):
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
    # Crates the browser runs: their files come from the backend in force,
    # which is a store in a tab and the disk on a desktop.
    if (re.search(r"\bstd::fs::", line)
            and str(rel).startswith(("crates/balaur_core/", "crates/balaur_render/",
                                     "crates/balaur_text/", "crates/balaur_ui/",
                                     "crates/balaur_script_rune/",
                                     "crates/balaur/"))
            and not is_test_file(rel)
            and "// os files:" not in line
            and not str(rel).endswith(("files.rs", "standalone.rs"))
            and rel.as_posix() != WHOLE_OWNER):
        out.append(Finding(rel, i, "std-fs",
                           "`std::fs` reaches a disk the web build has none of: read and write "
                           "through `files::backend`", "ERROR"))
    if (re.search(r"\.is_absolute\(\)", line)
            and not str(rel).endswith("files.rs")
            and not is_test_file(rel)):
        out.append(Finding(rel, i, "is-absolute",
                           "`Path::is_absolute` is gated on unix/wasi, so it answers false for "
                           "every path in the web build: use `files::rooted`", "ERROR"))
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
    MEASURED_FILES.append((rel, len(lines)))
    impl_name = ""

    if len(lines) > MAX_FILE_LINES:
        findings.append(Finding(rel, len(lines), "file-too-long",
                                f"{len(lines)} lines, limit {MAX_FILE_LINES}", "ERROR"))

    # `use std::sync::mpsc::{channel, ..}` makes the call site read `channel()`.
    mpsc_imported = any(
        re.search(r"use\s+std::sync::mpsc::(?:\{[^}]*\b)?(?:sync_)?channel\b", one)
        for one in lines
    )
    # A channel is inert; what must not run during a replay is the worker on the
    # far end. A file that names the guard has already answered for its channels.
    io_guarded = any(
        ("ExternalIo" in one) or ("replay::suppressed" in one) or ("replay::is_playing" in one)
        for one in lines
    )

    text = "\n".join(lines)
    placement_answered = {
        kind: any(answer in text for answer in answers)
        for kind, answers in PLACEMENT_ANSWERS.items()
    }

    # The rule below only binds a crate that keeps one.
    has_vocabulary = (
        (ROOT / "crates" / crate / "src" / "vocabulary.rs").exists()
        and rel.name != "vocabulary.rs"
        and not is_test_file(rel)
    )

    # `write` or `copy` from `std::fs` brought in by name, then called bare.
    bare_fs = {alias or name.split()[0]
               for found in BARE_FS_IMPORT.findall(text)
               for name, alias in ((m.group(0), m.group(1)) for m in BARE_FS_NAME.finditer(found))}
    opens_options = "OpenOptions::new" in text or "File::options" in text

    in_test_mod = False
    test_brace_depth = None
    test_attr_line = 0
    # Live `.borrow()`/`.borrow_mut()` bindings: (name, depth it was taken at).
    held: list[tuple[str, int]] = []
    depth = 0
    open_fn: OpenFn | None = None
    raw_closing: str | None = None
    comment_run_start = None
    comment_run: list[str] = []
    comment_run_is_doc = False

    for i, raw in enumerate(lines, start=1):
        line = raw.strip()

        if has_vocabulary and not in_test_mod and VOCABULARY_LITERAL.search(line):
            findings.append(Finding(rel, i, "vocabulary-literal",
                                    "a params key spelled at the call site; name it in "
                                    "the crate's vocabulary.rs", "ERROR"))

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
                comment_run_start = None
                comment_run = []

        if (not is_comment and rel.as_posix().startswith(EGUI_PAINT_SCOPE)
                and rel.as_posix() not in EGUI_PAINT_HOLDERS and not is_test_file(rel)):
            m = EGUI_PAINT.search(line)
            if m:
                findings.append(Finding(rel, i, "epaint-outside-paint",
                                        f"`{m.group(0)}` reaches egui's painter; paint through "
                                        "`widget::paint`", "ERROR"))

        if not is_comment and rel.as_posix() not in GLAM_BOUND:
            m = PLATFORM_FLOAT_RS.search(line)
            if m:
                findings.append(Finding(rel, i, "platform-float-math",
                                        f"`{m.group(0).lstrip('.').rstrip('(')}` routes to the "
                                        "platform libm and "
                                        "differs across operating systems; use `libm::` "
                                        "(DETERMINISM.md)", "ERROR"))

        if (re.search(r"#\[cfg\(test\)\]", line) or "#[test]" in line) and not in_test_mod:
            in_test_mod = True
            test_brace_depth = depth
            test_attr_line = i
        if is_test_file(rel):
            in_test_mod = True

        if not is_comment and line:
            if re.search(r"#\[allow\(", line) and "reason" not in attribute(lines, i):
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
                # A descriptive .expect("..") message is itself the justification;
                # bare .unwrap() and filler messages still need a comment.
                msg = re.search(r"\.expect\s*\(\s*\"([^\"]*)\"", line)
                FILLER = {"failed", "error", "should work", "unwrap", "todo", "oops", "!"}
                self_explaining = bool(msg) and len(msg.group(1)) >= 12 \
                    and msg.group(1).strip().lower() not in FILLER
                commented = "//" in raw or (i > 1 and lines[i - 2].strip().startswith("//"))
                if not (self_explaining or commented):
                    findings.append(Finding(rel, i, "unjustified-unwrap",
                                            "unwrap/expect outside tests needs a justification comment "
                                            "or a descriptive expect message", "ERROR"))

            # A file is written whole, through `balaur_core::whole`: written
            # where it stands, a crash leaves it cut short; beside it under a
            # name two writers or two targets share, one truncates another's.
            if (not in_test_mod and not is_comment
                    and "// whole:" not in raw
                    and rel.as_posix() not in (WHOLE_OWNER, FILES_OWNER)
                    and not scaffolding(rel)
                    and (IN_PLACE_WRITE.search(line)
                         or (bare_fs and not line.startswith("use ")
                             and any(call.group(1) in bare_fs
                                     for call in BARE_FS_CALL.finditer(line)))
                         or (opens_options and OPEN_OPTIONS.search(line)
                             and opens_in_place(lines, i)))):
                findings.append(Finding(rel, i, "in-place-write",
                                        "a file written where it stands (a `fs::write` or `fs::copy`, "
                                        "by path or called bare, a `File::create`, a truncating or "
                                        "writing `OpenOptions`, a `write_in_place`) is cut short by "
                                        "a crash mid-write: `balaur_core::whole::write`, "
                                        "`write_cached` or `copy_cached` (a cache or an output a run "
                                        "rebuilds), `whole::Batch` or a backend's `write_each` for "
                                        "files written in one step, `Whole::create` for a stream, or "
                                        "a backend's `write`/`write_cached`; a file appended to "
                                        "after, or a backend's own commit, says why with `// whole:` "
                                        "on the line", "ERROR"))
            if (not in_test_mod and not is_comment
                    and "// whole:" not in raw
                    and rel.as_posix() not in (WHOLE_OWNER, FILES_OWNER)
                    and hand_made_partial(line)):
                findings.append(Finding(rel, i, "hand-made-partial",
                                        "a file written beside its target and moved over it is "
                                        "`balaur_core::whole`'s: `Whole::create`/`Whole::private` "
                                        "then `finish`, `whole::write`, or a backend's "
                                        "`write`; a move that is no such write, a folder's "
                                        "or a file another program made, says why with "
                                        "`// whole:` on the line", "ERROR"))

            if not in_test_mod and TIMED_WAIT.search(line):
                commented = "//" in raw or (i > 1 and lines[i - 2].strip().startswith("//"))
                if not commented:
                    findings.append(Finding(rel, i, "timed-wait",
                                            "a sleep or a socket timeout outside tests wakes a thread "
                                            "on a timer; sleep on the event instead (AGENTS.md, Threads), "
                                            "or say on the line above why this is a frame cap or a "
                                            "deadline", "ERROR"))

            if not in_test_mod and rel.as_posix() != PLACEMENT_OWNER:
                m = PLACEMENT_BORROW.search(line)
                # A site says why it owes no stamp on its line or the one above.
                said = re.search(r"//.*\bstamp", raw, re.I) or (
                    i > 1 and re.search(r"^\s*//.*\bstamp", lines[i - 2], re.I))
                if m and not placement_answered[m.group(1)] and not said:
                    kind = m.group(1)
                    owed = (f"call `{PLACEMENT_STAMP[kind]}` after the write"
                            if kind in PLACEMENT_STAMP else
                            "write the local `Transform` or `Appearance` and stamp that")
                    findings.append(Finding(
                        rel, i, "placement-unstamped",
                        f"`&mut {kind}` is written in place and nothing in this file stamps it, "
                        "so the scene sync never places the node again and it stops moving on "
                        f"screen; {owed}, or say on the line above why no stamp is owed "
                        "(ARCHITECTURE.md, Frame schedule: `SceneSync` places only what changed)",
                        "ERROR"))

            # One door for structural writes outside the kernel;
            # a test sets a world up as it likes.
            if crate != "balaur_core" and ".world_mut()" in line and not in_test_mod:
                findings.append(Finding(rel, i, "world-write",
                                        "the world is written around the door: use Engine's spawn, "
                                        "spawn_node, insert, insert_one, remove_one, remove, "
                                        "reparent or despawn, and world() to change a component "
                                        "in place", "ERROR"))

            # ExternalIo is the recorded seam, and its worker `Sender` is
            # unreachable except through `start`, which does nothing while a
            # recording plays. The DAP server is the one sanctioned channel.
            if (crate != "balaur_core" and not str(rel).endswith("balaur_debug/src/dap.rs")
                    and not in_test_mod and not io_guarded):
                opened = re.search(r"std::sync::mpsc::(?:sync_)?channel\s*\(", line) or (
                    mpsc_imported and re.search(r"(?<![\w:])(?:sync_)?channel\s*\(", line)
                )
                if opened and not ("//" in raw or channel_justified(lines, i)):
                    findings.append(Finding(rel, i, "channel-outside-external-io",
                                            "this file opens a std::sync::mpsc channel and names neither "
                                            "ExternalIo nor replay::suppressed, so nothing stops its worker "
                                            "reaching the outside during a replay; go through ExternalIo, or "
                                            "say here why this channel never leaves the process "
                                            "(ARCHITECTURE, 'What determinism is still missing', 4)",
                                            "ERROR"))

            if not in_test_mod:
                m = BORROW_BINDING.match(raw)
                if m:
                    held.append((m.group(1), depth))
                for dropped in re.findall(r"\bdrop\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)", line):
                    held = [h for h in held if h[0] != dropped]
                call = REENTRANT_CALL.search(line)
                if held and call and not m:
                    names = ", ".join(f"`{n}`" for n, _ in held)
                    findings.append(Finding(
                        rel, i, "borrow-across-reentry",
                        f"{names} still borrows a RefCell here, and "
                        f"`{call.group(0).rstrip('(').strip()}` can re-enter the engine; "
                        "read what is needed, drop the borrow, then call", "ERROR"))

            # `log` records carry no fields, so nothing downstream can filter on
            # them; tracing-log bridges dependencies, our own code uses tracing.
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

        if not is_comment or raw_closing is not None:
            inside_raw = raw_closing is not None
            code, raw_closing = outside_raw(raw, raw_closing)
            if open_fn is None and not inside_raw and RS_FN.match(line):
                open_fn = OpenFn(i, depth)
            code = LITERAL.sub("", code)
            if open_fn:
                open_fn.feed(code)
            depth += code.count("{") - code.count("}")
            held = [h for h in held if h[1] <= depth]
            if open_fn and open_fn.ends(depth, code):
                findings.extend(open_fn.findings(rel, i))
                open_fn = None
            # The body opens on a later line than its attribute, so the scope
            # cannot end on the attribute line or on attributes stacked after it.
            if in_test_mod and test_brace_depth is not None and depth <= test_brace_depth \
                    and i > test_attr_line and not line.startswith("#["):
                if not is_test_file(rel):
                    in_test_mod = False
                    test_brace_depth = None

    return findings


SHOWCASE = ROOT / "scripts" / "showcase.sh"
TAKE = re.compile(
    r"^(?:shot|clip|screen|screen_clip|scene_shot|import_shot|import_clip|godot_clip)"
    r"\s+(\S+)", re.M)
FILED = re.compile(r"^(\d+\.\d+) (\S+)$", re.M)
# Two takes the file runs as functions rather than as a line of its own.
BESPOKE_TAKES = {"covers", "objects"}


def check_showcase() -> list[Finding]:
    """Every picture the showcase takes names the milestone its subject
    shipped in, and every milestone names a take. Without both halves a
    `--milestone` run silently skips a take, or keeps rendering one nothing
    asks for."""
    if not SHOWCASE.exists():
        return []
    text = SHOWCASE.read_text()
    lines = text.splitlines()
    # The list is the file's own head; the takes are what follows it.
    body = text[text.find("backup_examples"):]
    taken = set(TAKE.findall(body)) | BESPOKE_TAKES
    filed = {name for _, name in FILED.findall(text)}
    out = []
    for name in sorted(taken - filed):
        line = next((i + 1 for i, l in enumerate(lines) if f" {name} " in l or l.endswith(f" {name}")), 1)
        out.append(Finding(SHOWCASE, line, "showcase-milestone",
                           f"{name} is filed under no milestone", "ERROR"))
    for name in sorted(filed - taken):
        line = next((i + 1 for i, l in enumerate(lines) if l.endswith(f" {name}")), 1)
        out.append(Finding(SHOWCASE, line, "showcase-milestone",
                           f"{name} is filed under a milestone and taken by nothing", "ERROR"))
    return out


# N21: a setting or option that measures time, rate or distance names its
# unit. The words that mean a measure, and the suffixes that say which one.
MEASURE_WORDS = ("timeout", "delay", "duration", "interval", "jitter", "slop", "rate",
                 "dt", "frequency", "period", "latency")
UNIT_SUFFIXES = ("_seconds", "_ticks", "_hz", "_pixels", "_degrees", "_fps")
SCHEMA_KEY = re.compile(r"^([a-z_]+) = \{", re.M)
SCHEMA_BLOCK = re.compile(r'r#"(.*?)"#', re.S)


def check_setting_units(files: list[Path]) -> list[Finding]:
    out = []
    for path in files:
        if is_test_file(path.relative_to(ROOT)):
            continue
        text = path.read_text()
        if "define_group" not in text and "SCHEMA" not in text:
            continue
        for block in SCHEMA_BLOCK.finditer(text):
            body = block.group(1)
            if "type = " not in body or "help = " not in body:
                continue
            for key in SCHEMA_KEY.finditer(body):
                name = key.group(1)
                if name.endswith(UNIT_SUFFIXES):
                    continue
                if any(re.search(rf"(^|_){w}($|_)", name) for w in MEASURE_WORDS):
                    line = text.count("\n", 0, block.start(1) + key.start()) + 1
                    out.append(Finding(path, line, "setting-unit",
                                       f"`{name}` measures something and says no unit: "
                                       f"end it in one of {', '.join(UNIT_SUFFIXES)} (N21)", "ERROR"))
    return out


# N18 and N19, read off every theme a project or the editor ships.
HUES = ("red", "orange", "yellow", "green", "blue", "purple", "violet", "pink", "cyan",
        "teal", "magenta", "gray", "grey", "white", "black", "brown")
FAMILIES = ("primary", "secondary", "success", "warning", "danger")
FAMILY_SUFFIXES = ("", "_fill", "_fill_hover", "_text", "_bg")
# Theme tables that are not widget styles, and the per-state and per-class
# sub-tables a style may hold.
THEME_SECTIONS = {"colors", "sizes", "fonts", "hues", "mixes"}
THEME_STATES = {"hover", "active", "focus", "disabled", "checked", "touch", "pointer",
                "narrow", "medium", "wide", "short", "tall"}
# Theme keys with no widget property of the same name: a part of a widget,
# or how it moves between states, that no property of its own styles.
THEME_ONLY = {"icon_fill", "padding_x", "padding_y", "stroke_width", "arrow",
              "transition", "offset", "outline", "outline_width", "outline_offset",
              "shadows", "duration", "letter_spacing", "text_transform", "flex_direction"}


def theme_files() -> list[Path]:
    out = []
    for pattern in ("editor/themes/*.toml", "examples/*/themes/*.toml",
                    "editor/library/**/themes/*.toml"):
        out.extend(sorted(ROOT.glob(pattern)))
    return out


def token_findings(path: Path, line: int, name: str) -> list[Finding]:
    words = name.split("_")
    if any(h in words for h in HUES):
        return [Finding(path, line, "theme-token",
                        f"`{name}` names a hue; a token names its job (N18)", "ERROR")]
    family = next((f for f in FAMILIES if name == f or name.startswith(f + "_")), None)
    if family and name[len(family):] not in FAMILY_SUFFIXES:
        return [Finding(path, line, "theme-token",
                        f"`{name}`: a family takes {', '.join(s or '(bare)' for s in FAMILY_SUFFIXES)} "
                        "(N18)", "ERROR")]
    return []


def check_themes() -> list[Finding]:
    import tomllib
    api = ROOT / "docs" / "generated" / "api.json"
    if not api.exists():
        return []
    components = json.loads(api.read_text())["components"]
    widget = {key for name, schema in components.items()
               if name == "widget" or name.startswith("widget_") for key in schema}
    out = []
    for path in theme_files():
        text = path.read_text()
        lines = text.splitlines()
        try:
            doc = tomllib.loads(text)
        except tomllib.TOMLDecodeError:
            continue

        def line_of(key: str) -> int:
            return next((i + 1 for i, l in enumerate(lines) if l.strip().startswith(f"{key} =")), 1)

        for name in doc.get("colors", {}):
            out.extend(token_findings(path, line_of(name), name))

        def walk(table: dict, top: bool) -> None:
            for key, value in table.items():
                if isinstance(value, dict):
                    if not (top and key in THEME_SECTIONS):
                        walk(value, False)
                    continue
                if top or key in THEME_STATES:
                    continue
                if key not in widget and key not in THEME_ONLY:
                    out.append(Finding(path, line_of(key), "theme-key",
                                       f"`{key}` is no widget property; a theme key is the "
                                       "property it styles, spelled the same (N19)", "ERROR"))
                if isinstance(value, str) and key.endswith(("color", "fill", "stroke")) \
                        and re.fullmatch(r"[a-z][a-z0-9_]*", value):
                    out.extend(token_findings(path, line_of(key), value))

        walk(doc, True)
    return out


FEATURE_SETS = ROOT / "scripts" / "features.sh"
ANDROID_MANIFEST = ROOT / "crates" / "balaur_android" / "Cargo.toml"


def check_template_features() -> list[Finding]:
    """package_runtime.sh builds the Android template from GAME_FEATURES; the
    crate's default mirrors it so a plain `cargo build` is that template."""
    import tomllib
    game = re.search(r"^GAME_FEATURES=([a-z0-9_,-]+)$", FEATURE_SETS.read_text(), re.M)
    text = ANDROID_MANIFEST.read_text()
    default = tomllib.loads(text).get("features", {}).get("default", [])
    want = set(game.group(1).split(",")) if game else set()
    if want == set(default):
        return []
    line = next((i + 1 for i, l in enumerate(text.splitlines()) if l.startswith("default =")), 1)
    missing = ", ".join(sorted(want - set(default))) or "nothing"
    extra = ", ".join(sorted(set(default) - want)) or "nothing"
    return [Finding(ANDROID_MANIFEST, line, "template-features",
                    f"default is not scripts/features.sh's GAME_FEATURES: lacks {missing}, "
                    f"adds {extra}", "ERROR")]


UNIT_TEST = re.compile(r"#\[(?:[a-z_]+::)?test\]|#\[cfg\(test\)\]")


def target_sources(root: Path, entry: Path) -> list[Path]:
    """The files a target compiles: its entry and every `mod` reached from it."""
    seen: list[Path] = []
    todo = [entry]
    while todo:
        path = todo.pop()
        if path in seen or not path.exists():
            continue
        seen.append(path)
        here = path.parent if path.name in ("lib.rs", "main.rs", "mod.rs") else path.with_suffix("")
        for m in re.finditer(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+([a-z0-9_]+)\s*;",
                             path.read_text(), re.M):
            todo += [here / f"{m.group(1)}.rs", here / m.group(1) / "mod.rs"]
    return seen


def check_unit_test_targets() -> list[Finding]:
    """A lib or bin with no tests in its sources says `test = false`: nextest
    otherwise compiles the crate a second time, under cfg(test), and links an
    empty binary on every run. One that says it while holding tests skips them."""
    import tomllib
    out: list[Finding] = []
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        crate = manifest.parent
        text = manifest.read_text()
        toml = tomllib.loads(text)
        targets = []
        lib = toml.get("lib", {})
        if (crate / lib.get("path", "src/lib.rs")).exists():
            targets.append(("[lib]", lib, crate / lib.get("path", "src/lib.rs")))
        for b in toml.get("bin", []):
            targets.append((f"[[bin]] {b['name']}", b, crate / b.get("path", "src/main.rs")))
        for label, table, entry in targets:
            tested = any(UNIT_TEST.search(f.read_text()) for f in target_sources(crate, entry))
            off = table.get("test") is False
            rel = manifest.relative_to(ROOT)
            if off and tested:
                out.append(Finding(rel, 1, "unit-test-target",
                                   f"{label} says test = false, so its unit tests never run", "ERROR"))
            elif not off and not tested:
                out.append(Finding(rel, 1, "unit-test-target",
                                   f"{label} holds no unit tests: set test = false, or every "
                                   "test run compiles it twice and links an empty binary", "ERROR"))
    return out


def area_of(rel: Path) -> str:
    """Its crate, or its top folder outside `crates/`, and its language."""
    parts = rel.parts
    name = parts[1] if parts[0] == "crates" and len(parts) > 2 else parts[0]
    return f"{name} ({rel.suffix[1:]})"


def quantile(values: list[int], q: float) -> int:
    ordered = sorted(values)
    return ordered[int(q * (len(ordered) - 1))] if ordered else 0


def shape_row(name: str, files: int, lines: int, test_lines: int, fns: list[Measured]) -> str:
    sizes = [f.lines for f in fns]
    branches = [f.branches for f in fns]
    return (f"| {name} | {files} | {lines} | {test_lines} | {len(fns)} "
            f"| {quantile(sizes, 0.5)} · {quantile(sizes, 0.9)} · {max(sizes, default=0)} "
            f"| {quantile(branches, 0.9)} · {max(branches, default=0)} |\n")


def stats_markdown() -> str:
    """What the size rules measured, as a run summary: reported, never gated."""
    rows: dict[str, dict] = {}
    for rel, n in MEASURED_FILES:
        row = rows.setdefault(area_of(rel), {"files": 0, "lines": 0, "test_lines": 0, "fns": []})
        if is_test_file(rel):
            row["test_lines"] += n
        else:
            row["files"] += 1
            row["lines"] += n
    shipped = [f for f in MEASURED_FNS if not is_test_file(f.path)]
    for fn in shipped:
        rows[area_of(fn.path)]["fns"].append(fn)
    out = ["### Code shape\n\n",
           "Shipped code per crate or top folder, with its tests' lines beside it. "
           "Branches are what `fn-too-many-branches` counts.\n\n",
           "| Area | Files | Lines | Test lines | Fns | Fn lines p50 · p90 · max "
           "| Branches p90 · max |\n| --- | --: | --: | --: | --: | --: | --: |\n"]
    for name, row in sorted(rows.items()):
        out.append(shape_row(name, row["files"], row["lines"], row["test_lines"], row["fns"]))
    out.append(shape_row("**all**", sum(r["files"] for r in rows.values()),
                         sum(r["lines"] for r in rows.values()),
                         sum(r["test_lines"] for r in rows.values()), shipped))
    out.append(f"\n<details><summary>Nearest the limits: {MAX_FN_LINES} lines and "
               f"{MAX_FN_BRANCHES} branches a function, {MAX_FILE_LINES} lines a file"
               "</summary>\n\n| Function | Lines | Branches |\n| --- | --: | --: |\n")
    near = sorted(MEASURED_FNS, key=lambda f: -max(f.lines / MAX_FN_LINES,
                                                    f.branches / MAX_FN_BRANCHES))
    out.extend(f"| `{f.path}:{f.line}` | {f.lines} | {f.branches} |\n" for f in near[:10])
    out.append("\n| File | Lines |\n| --- | --: |\n")
    out.extend(f"| `{rel}` | {n} |\n" for rel, n in sorted(MEASURED_FILES, key=lambda f: -f[1])[:5])
    out.append("\n</details>\n")
    return "".join(out)


CTX: Context | None = None


def _start_worker(ctx: Context) -> None:
    global CTX
    CTX = ctx


def _checked(path: Path) -> tuple[list[Finding], list[Measured], list[tuple[Path, int]]]:
    """One file's findings and what the size rules measured, from a worker:
    the measurements are module state, which a worker's process keeps apart."""
    MEASURED_FNS.clear()
    MEASURED_FILES.clear()
    found = check_script(path) if path.suffix == ".bs" else check_file(path, CTX)
    return found, list(MEASURED_FNS), list(MEASURED_FILES)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fail-on-error", action="store_true")
    ap.add_argument("--reports", action="store_true", help="also print REPORT findings")
    ap.add_argument("--stats", metavar="FILE",
                    help="append a markdown table of function and file sizes to FILE")
    args = ap.parse_args()

    findings: list[Finding] = []
    files = rust_files()
    ctx = scan_context(files)
    # Each file is checked on its own, so they spread over every core; `map`
    # keeps the order the findings print in.
    with concurrent.futures.ProcessPoolExecutor(
            initializer=_start_worker, initargs=(ctx,)) as pool:
        for found, fns, sized in pool.map(_checked, files + source_files(".bs"), chunksize=16):
            findings.extend(found)
            MEASURED_FNS.extend(fns)
            MEASURED_FILES.extend(sized)
    findings.extend(check_showcase())
    findings.extend(check_setting_units(files))
    findings.extend(check_themes())
    findings.extend(check_template_features())
    findings.extend(check_unit_test_targets())
    if args.stats:
        with open(args.stats, "a", encoding="utf-8") as out:
            out.write(stats_markdown())

    errors = [f for f in findings if f.severity == "ERROR"]
    reports = [f for f in findings if f.severity == "REPORT"]

    for f in errors:
        print(f"ERROR  {f.path}:{f.line}  [{f.rule}] {f.message}")
    if args.reports:
        for f in reports:
            print(f"report {f.path}:{f.line}  [{f.rule}] {f.message}")

    print(f"\n{len(files)} files · {len(errors)} errors · {len(reports)} reports (--reports)")
    return 1 if (errors and args.fail_on_error) else 0


if __name__ == "__main__":
    sys.exit(main())
