#!/usr/bin/env python3
"""Write crates/balaur_script_rune/src/value/glam_api.rs from glam's source.

Every public method glam gives a type a script can hold is bound under glam's
own name, so the script API follows glam rather than a hand-kept list. A
method whose signature reaches a type scripts cannot hold is left out.

    python3 scripts/gen_glam_api.py
"""

import re
import subprocess
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/balaur_script_rune/src/value/glam_api.rs"

# glam type -> (script type, source file)
TYPES = {
    "DVec2": ("Vec2", "f64/dvec2.rs"),
    "DVec3": ("Vec3", "f64/dvec3.rs"),
    "DVec4": ("Vec4", "f64/dvec4.rs"),
    "DQuat": ("Quat", "f64/dquat.rs"),
    "DAffine2": ("Transform2d", "f64/daffine2.rs"),
    "DAffine3": ("Transform3d", "f64/daffine3.rs"),
    "I64Vec2": ("IVec2", "i64/i64vec2.rs"),
    "I64Vec3": ("IVec3", "i64/i64vec3.rs"),
}

# Written by hand in glam_types.rs: a checked inverse, and the operators.
HAND = {
    ("DAffine2", "inverse"),
    ("DAffine3", "inverse"),
}

# glam's cast names, spelled with the script's type names.
RENAME = {"as_dvec2": "as_vec2", "as_dvec3": "as_vec3", "as_dvec4": "as_vec4",
          "as_i64vec2": "as_ivec2", "as_i64vec3": "as_ivec3"}
# glam's own f32 and 32-bit casts, whose names the renames above now take.
SKIP_NAMES = {"as_vec2", "as_vec3", "as_vec3a", "as_vec4", "as_ivec2", "as_ivec3",
              "as_quat", "as_affine2", "as_affine3a", "map", "is_nan_mask",
              "is_finite_mask", "is_negative_mask"}

# The features the build turns on for glam, so these methods exist.
BUILT = {'#[cfg(feature = "f64")]', '#[cfg(feature = "i64")]'}

PRIMS = {"f64": "f64", "bool": "bool", "i64": "i64"}


def glam_dir():
    meta = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT))
    wanted = [p for p in meta["packages"] if p["name"] == "glam"]
    graph = {n["id"]: n for n in meta["resolve"]["nodes"]}
    glamx = next(p for p in meta["packages"] if p["name"] == "glamx")
    dep = next(d["pkg"] for d in graph[glamx["id"]]["deps"] if d["name"] == "glam")
    pkg = next(p for p in wanted if p["id"] == dep)
    return Path(pkg["manifest_path"]).parent / "src"


def split_top(text):
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "<([":
            depth += 1
        elif ch in ">)]":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def ty(t, owner):
    """(script type, 'prim' | 'glam' | 'euler' | 'array' | 'usize') or None."""
    t = t.strip().replace("crate::", "")
    if t == "Self":
        t = owner
    if t in TYPES:
        return TYPES[t][0], "glam"
    if t in PRIMS:
        return PRIMS[t], "prim"
    if t == "EulerRot":
        return "&str", "euler"
    if t == "usize":
        return "i64", "usize"
    m = re.fullmatch(r"\[(f64|i64); \d\]", t)
    if m:
        return f"Vec<{m.group(1)}>", "array"
    return None


def ret(t, owner):
    """(rust return type, fn(expr) -> script expr) or None."""
    t = t.strip()
    m = re.fullmatch(r"Option<(.+)>", t)
    if m:
        inner = ret(m.group(1), owner)
        if not inner:
            return None
        return f"Option<{inner[0]}>", lambda e, f=inner[1]: f"{e}.map(|v| {f('v')})".replace(
            "|v| " + inner[0] + "::of(v)", inner[0] + "::of")
    if t.startswith("(") and t.endswith(")"):
        parts = [ret(p, owner) for p in split_top(t[1:-1])]
        if not all(parts):
            return None
        rust = "(" + ", ".join(p[0] for p in parts) + ")"
        return rust, lambda e, ps=parts: "{ let r = " + e + "; (" + ", ".join(
            p[1](f"r.{i}") for i, p in enumerate(ps)) + ") }"
    mapped = ty(t, owner)
    if not mapped or mapped[1] == "euler":
        return None
    name, kind = mapped
    if kind == "glam":
        return name, lambda e, n=name: f"{n}::of({e})"
    if kind == "array":
        return name, lambda e: f"{e}.to_vec()"
    if kind == "usize":
        return name, lambda e: f"i64::try_from({e}).unwrap_or(i64::MAX)"
    return name, lambda e: e


def signatures(src, owner):
    text = src.read_text()
    start = text.index(f"\nimpl {owner} {{")
    body = text[start:]
    end = re.search(r"\n}\n", body).start()
    body = body[:end]
    lines = body.split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        m = re.match(r"    pub (const )?fn (\w+)", line)
        if not m:
            i += 1
            continue
        attrs = []
        j = i - 1
        # Back to the previous item: an attribute may span lines.
        while j >= 0 and lines[j].strip() and not lines[j].strip().endswith(("}", "{", ";")):
            attrs.append(lines[j].strip())
            j -= 1
        sig = line
        while "{" not in sig and " where" not in sig:
            i += 1
            sig += " " + lines[i].strip()
        i += 1
        if any(a.startswith("#[deprecated") or (a.startswith("#[cfg(") and a not in BUILT)
               for a in attrs):
            continue
        yield m.group(2), sig


def emit(owner, name, sig, out):
    script = TYPES[owner][0]
    if name in SKIP_NAMES or (owner, name) in HAND or "<" in sig.split("(")[0]:
        return
    m = re.search(r"fn \w+\((.*?)\)\s*(->\s*(.+?))?\s*(\{|where)", sig)
    if not m:
        return
    params = split_top(m.group(1))
    rtype = (m.group(3) or "()").strip()
    method = False
    args, calls = [], []
    for p in params:
        p = p.replace("mut ", "")
        if p in ("self", "&self"):
            method = True
            args.append(f"this: &{script}")
            continue
        if p.startswith("&"):
            return
        pname, ptype = [s.strip() for s in p.split(":", 1)]
        mapped = ty(ptype, owner)
        if not mapped or mapped[1] in ("array", "usize"):
            return
        sname, kind = mapped
        if kind == "glam":
            args.append(f"{pname}: &{sname}")
            calls.append(f"{pname}.g()")
        elif kind == "euler":
            args.append(f"{pname}: &str")
            calls.append(f"vm_try!(euler_order({pname}))")
        else:
            args.append(f"{pname}: {sname}")
            calls.append(pname)
    if rtype == "()":
        return
    back = ret(rtype, owner)
    if not back:
        return
    rust, wrap = back
    target = "this.g()." if method else f"{owner}::"
    call = wrap(f"{target}{name}({', '.join(calls)})")
    uses_vm = "vm_try!" in call
    script_name = RENAME.get(name, name)
    closure = f"|{', '.join(args)}|"
    if uses_vm:
        body = f"VmResult::Ok({call})"
        rtype_out = f"VmResult<{rust}>"
    else:
        body = call
        rtype_out = rust
    if method:
        out.append(f'    m.associated_function("{script_name}", {closure} -> {rtype_out} {{ {body} }})?;')
    else:
        out.append(f'    m.function("{script_name}", {closure} -> {rtype_out} {{ {body} }})')
        out.append(f"        .build_associated::<{script}>()?;")


def constants(src, owner, out):
    script = TYPES[owner][0]
    text = src.read_text()
    body = text[text.index(f"\nimpl {owner} {{"):]
    for m in re.finditer(r"\n    pub const ([A-Z_0-9]+): Self =", body):
        out.append(f'    m.constant("{m.group(1)}", {script}::of({owner}::{m.group(1)}))')
        out.append(f"        .build_associated::<{script}>()?;")


def snake(script):
    """`Transform2d` as a Rust item name: the dimension is its own word."""
    return re.sub(r"(\d)d$", r"_\1d", script.lower())


def main():
    src = glam_dir()
    head = [
        "//! Written by `scripts/gen_glam_api.py` from glam's source: every method",
        "//! glam gives these types, under glam's own name. Edit the script, not this.",
        "",
        "#![allow(",
        "    clippy::trivially_copy_pass_by_ref,",
        '    reason = "one generated registration per glam method"',
        ")]",
        "",
        "use glamx::glam::{DAffine2, DAffine3, DQuat, DVec2, DVec3, DVec4, I64Vec2, I64Vec3};",
        "use rune::runtime::VmResult;",
        "use rune::vm_try;",
        "",
        "use super::glam_types::{Glam as _, IVec2, IVec3, Quat, Transform2d, Transform3d, Vec4, euler as euler_order};",
        "use super::{Vec2, Vec3};",
        "",
    ]
    parts, body = [], []
    for owner, (script, file) in TYPES.items():
        lines = []
        constants(src / file, owner, lines)
        for name, sig in signatures(src / file, owner):
            emit(owner, name, sig, lines)
        # One function per 90 lines keeps each under the house limit.
        chunk, n = [], 0
        for line in lines:
            chunk.append(line)
            if len(chunk) >= 90 and not line.endswith(")"):
                n += 1
                parts.append((f"{snake(script)}_{n}", chunk))
                chunk = []
        if chunk:
            n += 1
            parts.append((f"{snake(script)}_{n}", chunk))
    body.append("pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {")
    for name, _ in parts:
        body.append(f"    {name}(m)?;")
    body += ["    Ok(())", "}", ""]
    for name, chunk in parts:
        # One call per line is what keeps a chunk inside the house line limit;
        # rustfmt would wrap each into three and blow past it.
        body.append("#[rustfmt::skip]")
        body.append(f"fn {name}(m: &mut rune::Module) -> Result<(), rune::ContextError> {{")
        body += chunk
        body += ["    Ok(())", "}", ""]
    OUT.write_text("\n".join(head + body))
    # Written in whatever shape the loops above leave; CI's rustfmt job reads
    # this file like any other.
    subprocess.run(["rustfmt", "--edition", "2024", str(OUT)], check=True)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    sys.exit(main())
