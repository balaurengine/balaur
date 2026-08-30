> **Status:** superseded in part. This was written before the scripting seam
> existed and says Lua/Luau was rejected; both Luau and Rune ship, selected per
> project. Kept for the reasoning, not as a description of the code.

# Plan: extensibility tiers and the quality apparatus

Draft for review. Two parts: how code is layered and loaded, and how we keep
code we cannot read by hand.

## 0. Blocking decisions

These are settled in the design handbook but contradicted by the code. Building
a module/extension API on an unsettled core means designing the seam twice.

| Question | Handbook | Code | Cost of deciding late |
|---|---|---|---|
| Scripting language | Rune (Lua/Luau rejected, 1-based) | `mlua 0.12` + `luau` | Every binding and every shipped script |
| Data plane | Node tree, columnar, not an ECS (marked irreversible) | `hecs 0.11` + node facade | The extension API surface: components vs nodes |
| Audio | `kira` | `rodio` | Small |

The extension API cannot be specified until the second row is answered, because
it decides whether a third party registers *components and systems* or *node
classes*.

## 1. The four tiers

```
  ┌──────────────────────────────────────────────────────────┐
  │ SCRIPTS      .rn / .luau — per node, hot reloaded         │  no build step
  ├──────────────────────────────────────────────────────────┤
  │ EXTENSIONS   third-party cdylib, loaded at runtime        │  dlopen, C ABI
  │              self-registering, may depend on other exts   │
  ├──────────────────────────────────────────────────────────┤
  │ MODULES      ours, compile-time on/off for size and perf  │  cargo feature
  ├──────────────────────────────────────────────────────────┤
  │ CORE         balaur_core — always present                 │  always linked
  └──────────────────────────────────────────────────────────┘
```

| Tier | Written by | Linked | Removable | API |
|---|---|---|---|---|
| Core | us | always | no | Rust, internal |
| Module | us | compile time | cargo feature | **the plugin API** |
| Extension | anyone | run time | drop the file | **the plugin API, over C ABI** |
| Script | users | never | per file | the scripting host |

## 2. One API for modules and extensions

Godot has this split and got it wrong in a specific way worth avoiding: modules
(C++, compiled in, full internal access) and GDExtension (C ABI, public API
only) are **different APIs**, so a module cannot become an extension without a
rewrite. We should not repeat that.

**The rule that makes them one thing: the plugin API must be expressible over a
C ABI.** A module then uses it directly; an extension uses it through `dlopen`.
Same trait, two link strategies.

```rust
// The whole contract. Modules implement it directly.
pub trait Plugin {
    fn manifest(&self) -> &Manifest;         // name, semver, deps, abi_version
    fn register(&mut self, reg: &mut Registry);
}

// Extensions export exactly one symbol; the loader wraps it as a `Plugin`.
#[no_mangle]
pub extern "C" fn balaur_plugin_entry(abi: u32) -> *mut PluginVTable;
```

Consequences, stated up front because they constrain core too:

- The registry is **opaque handles and `extern "C"` functions**. No generics, no
  Rust trait objects, no `&mut World` handed across the boundary.
- Anything a module wants from core has to exist in the C API. That is a real
  tax on core's design, and it is the price of one API instead of two.
- Extension-to-extension dependencies resolve **through the registry by name**,
  not by dynamic linking, so there is no symbol soup and no load-order fragility
  beyond the manifest.
- Load order is a topological sort of the manifest graph, **broken by sorting on
  name**, never on directory iteration order. Directory order is not
  deterministic and would leak into the state hash.
- `abi_version` is checked on load and refused on mismatch, with the version in
  the error.

**Open question for review:** does a module get to skip the C ABI when compiled
in? Skipping is faster and more ergonomic; not skipping is what guarantees a
module can ship as an extension unchanged. Recommendation: do not skip. Measure
the cost first — if a registry call is nanoseconds and happens at registration
rather than per frame, there is nothing to optimise.

## 3. Quality apparatus

The premise: this codebase will be mostly machine-written and cannot be read
line by line. Every rule below exists to replace a review we will not do.

### 3.1 Custom lints

Off-the-shelf lints do not catch how AI writes badly. These are house rules,
enforced in CI, failing the build.

| Rule | Why |
|---|---|
| Comment block longer than N lines | Verbosity is the most consistent failure |
| Comment that only restates the identifier | *"// increments the counter"* over `increment_counter` — implement by tokenising the identifier and the comment and flagging high overlap; the finding goes to a human or an AI reviewer to confirm, not straight to a failure |
| `unwrap`/`expect` outside tests without a justification comment | |
| `#[allow(..)]` without a reason string | |
| `TODO`/`FIXME` without an issue link | |
| Function over N lines, file over M lines | |
| `HashMap`/`HashSet` iteration in a sync path | Determinism, already a design rule |
| Magic numeric literal outside a `const` | |

Tooling: `dylint` for lints needing type information, `ast-grep` for
syntactic pattern rules (YAML, fast, no rustc). Start with `ast-grep` — most of
the list above is syntactic.

**Rule for adding rules:** when the AI does something bad twice, it becomes a
lint. The lint file is the memory.

### 3.2 CI, green from day one

No warnings allowed, no skipped jobs, no `continue-on-error`.

| Gate | Tool |
|---|---|
| Format | `cargo fmt --check` |
| Lint | `cargo clippy -- -D warnings` + house lints |
| Test | `cargo nextest`, per crate |
| Unsafe | `cargo miri` on core |
| Licences | `cargo deny` — the tree is not pure MIT |
| Advisories | `cargo audit` |
| Determinism | state-hash test: N frames twice, assert equal |
| Docs in sync | regenerate, `git diff --exit-code` |
| Perf | benchmark suite, fail on >X% regression |
| E2E | headless run of every example, assert exit and frame count |
| Observability | run a flow, assert the expected log events appear |

Observability tests need structured logging first: `tracing` with a test
subscriber that captures events, so a test can assert *"a reload of `pig.luau`
emitted `script.reload` with `nodes=24`"*. That is a core change, not a test
change, and it should land before the tests that depend on it.

### 3.3 Generated documentation

Hand-written docs go stale. Everything below is generated and diffed in CI, so
it is either correct or the build is red.

Per component, generated from source:

- component purpose and public surface, from rustdoc JSON
- the types it owns and what each is for
- its dependencies, as a Mermaid graph from the crate manifests

Per product, generated from annotations plus source:

- user flows — from `#[flow(..)]` annotations on entry points, or from the e2e
  test names, so a flow that is not tested does not appear
- component architecture — the crate graph
- system architecture — what runs where: process, thread, device, and whether it
  touches the network or the filesystem

**The systems view has a second purpose:** it tells an AI, and us, whether a
given component is even allowed to open a socket or read a file. It is a
capability map, not just a diagram.

### 3.4 Performance and flow tests

- Micro-benchmarks per subsystem, checked against a budget, not just tracked.
- Flow tests that drive a real example end to end and assert frame timings.
- The budget table lives with the docs and is regenerated, so a regression shows
  up as a diff rather than a number nobody reads.

## 4. Sequencing

Nothing here is useful until the blocking decisions land.

| | Work | Depends on |
|---|---|---|
| 0 | Settle Luau-vs-Rune and hecs-vs-nodes | — |
| 1 | CI skeleton: fmt, clippy, nextest, deny, audit — all green | — |
| 2 | `tracing` through core, structured events | — |
| 3 | House lint set, starting with comment rules | 1 |
| 4 | Doc generator + CI diff gate | 1 |
| 5 | Manifest, `Registry`, `Plugin` trait; convert one existing crate to a module | 0 |
| 6 | C ABI + loader; ship one extension out of tree | 5 |
| 7 | Extension-to-extension deps, topological load | 6 |
| 8 | Perf budgets, e2e, observability tests | 1, 2 |

Items 1 to 4 are independent of the blocking decisions and can start now.
