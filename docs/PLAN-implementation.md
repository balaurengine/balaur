> **Status:** phases 0-4 are done. Phase 5 (modules and extensions) is not
> started. Kept as written for the record; it is a plan, not a description of
> the code. See [docs/generated/](generated/) for what the code actually is.

# Implementation plan

Ordered least-intrusive first. Phases 0-2 touch no engine code and can start
immediately. Phase 3 onward changes the engine.

Decisions taken: **both Lua and Rune**; **hecs stays**, revisit later;
**Rust-only modules**, buildable static or as a dylib.

## Phase 0 — CI, green from the first commit

No engine changes. Today there is no `.github/workflows` at all on ~7,500 lines.

| Gate | Command | Notes |
|---|---|---|
| Toolchain pin | `rust-toolchain.toml` | Not present. `wesl` (via kiss3d) needs 1.97.1, egui 1.95 — a default toolchain cannot build this repo |
| Format | `cargo fmt --check` | |
| Lint | `cargo clippy --all-targets -- -D warnings` | |
| Test | `cargo nextest run` | measured ~35% faster than `cargo test` |
| Licences | `cargo deny check` | tree is not pure MIT: rapier/parry/winit Apache-2.0-only, kiss3d BSD-3 |
| Advisories | `cargo deny check advisories` | |
| Build examples | `cargo build --examples` | `examples/angrynerds` is the canary |

Rules: no `continue-on-error`, no skipped jobs, no allowed warnings. Use
`actions/checkout@v7`, `setup-node@v7` (node24 — v4 is deprecated).

**Exit:** every gate green on `main`.

## Phase 1 — House lints

No engine changes. These encode failure modes rather than taste, and the rule
for adding one is: **when the same bad pattern appears twice, it becomes a
lint.**

Tooling: `ast-grep` for syntactic rules (YAML, no rustc, fast). `dylint` later
for anything needing type information.

| Rule | Kind |
|---|---|
| Comment block over N lines | hard fail |
| Comment that mostly restates the identifier it sits on | **report, not fail** |
| `unwrap`/`expect` outside `#[cfg(test)]` without a justification comment | hard fail |
| `#[allow(..)]` without a reason string | hard fail |
| `TODO`/`FIXME` without an issue link | hard fail |
| Function over N lines, file over M lines | hard fail |
| `HashMap`/`HashSet` iteration in a frame path | hard fail — determinism |
| Bare numeric literal outside a `const` | report |

The comment-quality rule is a heuristic: tokenise the identifier, tokenise the
comment, flag high overlap. It will false-positive on short honest comments, so
it produces findings for a human or an AI pass to confirm. Failing the build on
a heuristic teaches people to write worse comments to satisfy it.

**Exit:** `just lint` runs locally and in CI; the rule file is the memory.

## Phase 2 — Generated architecture docs

No engine changes. Hand-written docs go stale; these are regenerated and
`git diff --exit-code` in CI, so they are correct or the build is red.

Generated per crate, from rustdoc JSON and the manifests:

- purpose, public surface, the types it owns
- its dependency edges, as a Mermaid graph

Generated for the product:

- crate graph
- **systems view**: what runs in which process and thread, and which components
  touch the network or the filesystem. This doubles as a capability map — it is
  what tells us, and an AI, whether a given component has any business opening a
  socket.
- user flows, derived from e2e test names, so an undocumented flow is simply an
  untested one

**Exit:** `just docs` regenerates; CI fails on drift.

## Phase 3 — The scripting seam

First engine change, and additive. **This is the prerequisite for Rune**: today
`mlua` types appear in 20 files across 6 crates, so adding a second language
without a seam means writing every binding twice.

Current coupling, measured:

| Crate | `mlua`/`Lua` refs | Files |
|---|---|---|
| balaur_core | 88 | 11 |
| balaur_ui | 23 | 3 |
| balaur_physics | 14 | 2 |
| balaur_render | 7 | 1 |
| balaur_audio | 3 | 1 |
| balaur_input | 2 | 1 |

Only `balaur_core` declares the dependency; the rest see it by re-export. That
is what makes this tractable.

Steps:

1. New crate `balaur_script`: traits only, no backend. `ScriptLanguage`,
   `ScriptInstance`, a neutral `Value`, and a `Bindings` registry that a
   subsystem calls to declare functions and types once.
2. Move the ~50 subsystem references onto `Bindings`. Each crate stops naming a
   language.
3. New crate `balaur_script_luau`: the only crate that names `mlua`. Move
   core's Luau-specific half (`script/env.rs`, `script/node_api.rs`, parts of
   `script/mod.rs`) into it.
4. `balaur_script_mock`: a deliberately awkward stub backend, CI-only. A trait
   with one implementation is not an abstraction — this is what keeps the seam
   honest, and it costs a day.

**Exit:** `cargo test` green with Luau behind the trait; `examples/angrynerds`
unchanged and running.

## Phase 4 — Rune as the second backend

Only sensible after Phase 3.

1. `balaur_script_rune` implementing the same traits.
2. Per-node model matching Luau's: today a `.luau` file returns a class table
   and each node gets an instance table. Rune's equivalent needs deciding —
   likely a module per file plus a state object per node.
3. Language chosen per file extension, both loadable in one project.
4. Port `examples/angrynerds` scripts to Rune as the conformance test. Two
   languages running the same game is the proof the seam works.

Known gap: **Rune has no debugger.** That cost is real and unbudgeted.

**Exit:** the same example runs under either backend, and under both at once.

## Phase 5 — Modules and extensions

Rust-only, one `Plugin` trait, static or dylib.

```rust
pub trait Plugin {
    fn manifest(&self) -> &Manifest;      // name, semver, deps, build fingerprint
    fn register(&mut self, reg: &mut Registry);
}
```

- **Module**: implements `Plugin`, linked in, on/off by cargo feature.
- **Extension**: the same trait, built as a `dylib`, loaded at run time.

**The catch that decides this phase.** Rust has no stable ABI. A Rust dylib
passing Rust types across `dlopen` requires the *identical* rustc version and
flags on both sides; mismatch is undefined behaviour, not a clean error. Two
honest options:

1. **Fingerprint and refuse.** Embed rustc version, engine semver and a hash of
   the `balaur_script`/`Registry` API in the manifest; refuse to load on
   mismatch with the versions in the message. Extensions must be rebuilt for
   every engine release. Simple, safe, restrictive.
2. **C ABI at the boundary.** Portable across toolchains, but constrains the
   registry to opaque handles and `extern "C"` functions, which taxes core's
   design too.

Recommendation: **(1) now, (2) before any third party ships an extension.**
Design the `Registry` so it *could* be expressed over a C ABI — no generics, no
Rust trait objects in the signatures — so option 2 is later work, not a rewrite.

Steps:

1. `Manifest`, `Registry`, `Plugin` in a new `balaur_plugin` crate.
2. Convert `balaur_audio` (106 lines, smallest) to a module. Tracer bullet.
3. Feature-gate it; prove the binary shrinks with it off.
4. Dylib loading plus the fingerprint check.
5. One extension built out of tree, loaded at run time.
6. Extension-to-extension dependencies: manifest graph, topological load,
   **ordered by name** — directory iteration order is not deterministic and
   would leak into the state hash.

**Exit:** audio ships as a module or an extension from the same source, and can
be switched off.

## Order and independence

```
  Phase 0  CI            ──┐
  Phase 1  lints         ──┼── no engine changes, start now, any order
  Phase 2  doc gen       ──┘

  Phase 3  script seam   ─── prerequisite for 4
  Phase 4  Rune          ───
  Phase 5  modules       ─── independent of 3/4, but easier after 3
```

Phase 5 does not depend on Phases 3-4, but doing the script seam first means the
plugin API is designed with one working example of a seam already in the tree.

Not in this plan: the hecs-versus-node-model question, deferred by decision.
It stays a risk because it decides whether an extension registers *components
and systems* or *node classes* — and that is the plugin API's shape.
