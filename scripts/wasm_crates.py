#!/usr/bin/env python3
"""Charge every function body of a wasm module to the crate that defines it.

Usage: wasm_crates.py module.wasm [--top N] [--crate NAME]

The module must keep its `name` section: build with
`CARGO_PROFILE_WEB_STRIP=false`, run `wasm-bindgen --keep-debug`, then
`wasm-opt -Oz -g --strip-dwarf`. Sizes are code bytes; `shipped` is the module
less its name section, which `wasm-opt --strip-debug` removes.
"""
import re
import sys
from collections import defaultdict

HASH = re.compile(r"\[[0-9a-f]+\]")


def leb(buf, i):
    result = shift = 0
    while True:
        b = buf[i]
        i += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            return result, i
        shift += 7


def sections(buf):
    i = 8
    while i < len(buf):
        sid = buf[i]
        size, j = leb(buf, i + 1)
        yield sid, buf[j:j + size]
        i = j + size


def demangle_legacy(name):
    if not name.startswith("_ZN"):
        return name
    i, parts = 3, []
    while i < len(name) and name[i] != "E":
        m = re.match(r"\d+", name[i:])
        if not m:
            break
        n = int(m.group())
        i += len(m.group())
        parts.append(name[i:i + n])
        i += n
    if parts and re.fullmatch(r"h[0-9a-f]{16}", parts[-1]):
        parts.pop()
    text = "::".join(parts)
    for a, b in (("$LT$", "<"), ("$GT$", ">"), ("$u20$", " "), ("$RF$", "&"), ("$C$", ","),
                 ("$BP$", "*"), ("$u7b$", "{"), ("$u7d$", "}"), ("$u27$", "'"), ("$u5b$", "["),
                 ("$u5d$", "]"), ("$u3b$", ";"), ("$u21$", "!"), ("..", "::")):
        text = text.replace(a, b)
    return text


def readable(name):
    # wasm-bindgen writes v0 names demangled, with each crate's hash in brackets.
    return HASH.sub("", demangle_legacy(name))


def crate_of(name):
    n = readable(name)
    # `<T as Trait>::f` and `<T>::f` belong to T's crate.
    while n.startswith("<"):
        n = n[1:]
        n = n.lstrip("&*").replace("mut ", "", 1) if n.startswith(("&", "*")) else n
    m = re.match(r"(?:dyn )?([A-Za-z_][A-Za-z0-9_]*)::", n)
    return m.group(1) if m else "?"


def skip_import(sec, i):
    for _ in range(2):
        n, i = leb(sec, i)
        i += n
    kind = sec[i]
    i += 1
    if kind in (1, 2):
        if kind == 1:
            i += 1
        flags, i = leb(sec, i)
        _, i = leb(sec, i)
        if flags & 1:
            _, i = leb(sec, i)
    elif kind == 3:
        i += 2
    else:
        _, i = leb(sec, i)
    return kind == 0, i


def read(buf):
    imports, bodies, names, data, named = 0, [], {}, 0, 0
    for sid, sec in sections(buf):
        if sid == 2:
            count, i = leb(sec, 0)
            for _ in range(count):
                is_function, i = skip_import(sec, i)
                imports += is_function
        elif sid == 10:
            count, i = leb(sec, 0)
            for _ in range(count):
                size, j = leb(sec, i)
                bodies.append(size + (j - i))
                i = j + size
        elif sid == 11:
            data = len(sec)
        elif sid == 0:
            n, i = leb(sec, 0)
            if sec[i:i + n] != b"name":
                continue
            named = len(sec)
            i += n
            while i < len(sec):
                sub = sec[i]
                size, j = leb(sec, i + 1)
                if sub == 1:
                    count, k = leb(sec, j)
                    for _ in range(count):
                        idx, k = leb(sec, k)
                        ln, k = leb(sec, k)
                        names[idx] = sec[k:k + ln].decode("utf-8", "replace")
                        k += ln
                i = j + size
    return imports, bodies, names, data, named


def main():
    path = sys.argv[1]
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    only = sys.argv[sys.argv.index("--crate") + 1] if "--crate" in sys.argv else None
    buf = open(path, "rb").read()
    imports, bodies, names, data, named = read(buf)
    if not names:
        raise SystemExit(f"{path} has no name section; see the usage")
    per = defaultdict(int)
    funcs = defaultdict(list)
    for n, size in enumerate(bodies):
        name = names.get(imports + n, "")
        crate = crate_of(name) if name else "?"
        per[crate] += size
        funcs[crate].append((size, readable(name)))
    mib = lambda b: b / 1048576
    print(f"shipped {mib(len(buf) - named):.2f} MiB: code {mib(sum(bodies)):.2f}, "
          f"data {mib(data):.2f}, functions {len(bodies)}")
    if only:
        for size, name in sorted(funcs[only], reverse=True)[:top]:
            print(f"{size:>9} {name[:150]}")
        return
    for crate, size in sorted(per.items(), key=lambda kv: -kv[1])[:top]:
        print(f"{mib(size):7.3f} {crate}")


main()
