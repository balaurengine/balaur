# Quality

Nothing here depends on remembering. Every rule below is a script that fails,
and every script runs on every push and every pull request.

`AGENTS.md` and `docs/NAMING.md` say what the rules are. This file says what
enforces them, and where.

## One entry point

`.github/workflows/runner.yml` owns the triggers — a push to `main`, a `v*`
tag, and every pull request — and calls four reusable workflows. Splitting
them means a red X names itself before you open it.

| Workflow | What it covers |
| --- | --- |
| `lint.yml` | everything that reads the code without running it |
| `docs.yml` | both kinds of documentation |
| `test.yml` | everything that runs the engine |
| `build.yml` | everything that produces a download |

`build.yml` runs on pull requests too; only its one publishing job is skipped,
so a PR still proves that every platform builds and every export works.

## Before you push

```bash
scripts/lint.sh
```

Format, three clippy sweeps, the three lint scripts and the notices check, in
the order CI runs them. Install it as a pre-push hook once:

```bash
git config core.hooksPath .githooks
```

Green locally and red on push means the two have drifted, which is a bug in
the scripts, not in the commit.

## The compiler, per platform and per feature

The toolchain is pinned in `rust-toolchain.toml` to the dependency tree's MSRV,
with `rustfmt` and `clippy` as components, so every machine runs the same
linter version and an older toolchain fails with a message that names the
cause.

- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets -- -D warnings` on Linux, macOS and
  Windows — `#[cfg(windows)]` code is compiled nowhere else.
- Then once per feature that is off by default, because code behind an off
  feature is not compiled at all: `window` (kiss3d, wgpu, egui and the macOS
  dock-icon path), `extensions` (the dlopen path and the cdylib), and `apple`
  (Game Center, StoreKit, objc2 — macOS only).
- And `examples/extension_greeter`, deliberately outside the workspace: the
  only thing proving an extension builds without the engine's build tree.

## House rules no compiler enforces

`scripts/house_lints.py` walks every `.rs` and `.rn` in the tree. Two
severities: **ERROR** fails CI and is mechanical; **REPORT** prints only, for
heuristics that will false-positive — failing a build on a heuristic teaches
people to game the heuristic.

| Rule | Fails on |
| --- | --- |
| `platform-float-math` | `.sin()`, `f32::sin(x)`, `.powf()` and the rest of the inexact list; `sqrt`, `abs`, `floor`, `round` and friends are IEEE-exact and deliberately absent |
| `nondeterministic-iteration` | iterating a `HashMap`/`HashSet`; `DetHashMap`/`DetHashSet` are the sanctioned substitutes |
| `channel-outside-external-io` | a file that opens an `mpsc` channel and names neither `ExternalIo` nor `replay::suppressed` — nothing would stop its worker reaching the outside during a replay |
| `allow-without-reason` | an `#[allow(..)]` with no `reason = ".."` and no comment above it |
| `unjustified-unwrap` | `unwrap`/`expect` outside tests with no justification comment and no descriptive message |
| `log-instead-of-tracing` | `log::*` in our own code; its records carry no fields for anything downstream to filter on |
| `todo-without-issue` | a TODO or FIXME with no issue behind it |
| `fn-too-long`, `file-too-long` | 120 lines, 1200 lines |
| `comment-too-long`, `comment-restates-name` | comment blocks and comments that say what the line below already says |
| `det-prefix-misuse`, `dimension-casing`, `dimension-snake`, `install-verb`, `system-verb`, `engine-param-name`, `resource-suffix`, `new-resource-type`, `fn-suffix-on-struct`, `pub-inner`, `component-registration-doc` | the mechanical half of `docs/NAMING.md` |
| `rune-short-circuit`, `rune-rebound-let` | two Rune shapes that compile and then misbehave — a field assignment whose `\|\|` overwrites the local, and an `if let` that rebinds a name to itself |

**The ratchet.** `scripts/house_lints_baseline.txt` records, per file and per
rule, how many violations predate the rule. Anything above that count is new
and fails. Counts rather than line numbers, so the baseline survives edits
above them. `--debt` prints what is outstanding; deleting a line from it is
progress, and nothing may be added by hand.

**The rule for adding a rule:** when the same bad pattern shows up twice, it
becomes a lint. The file is the memory.

## Comments

`scripts/comment_lints.py` covers every language in the tree, not just Rust:
`.rs`, `.rn`, `.py`, `.sh`, `.yml`, `.toml`. Generated files skip themselves
by their banner.

- **essay** — more than three consecutive plain-comment lines. A shell
  script's leading block is its doc comment and gets eight. (`house_lints.py`
  keeps a looser absolute cap of twelve.)
- **restates-code** — a comment whose words add nothing to the line under it.
- Banners and dividers; the structure is the divider.

A comment is one or two lines stating a constraint. If the reason needs a
paragraph it is architecture, and it goes in `ARCHITECTURE.md`.

## Names

`docs/NAMING.md` holds sixteen rules, each tagged with a scope and the cost of
getting it wrong: `rust-internal` is compiler-caught, `script-api` breaks
existing projects at `balaur export`, `scene-file` breaks existing `.toml`
scenes and the inspector generated from the same schemas. Names are the part
of the engine that cannot be refactored later without breaking someone's
project, so they are decided once and checked mechanically where possible.

`house_lints.py` covers the Rust half. `scripts/api_lints.py` covers the
script API — by **booting the engine** and reading what `balaur api` prints,
not by parsing source: derived constants like `input.KEY_SPACE` exist only at
registration time, and a name scripts cannot reach is not API.

| Check | Fails on |
| --- | --- |
| `getter-prefix` | a reader named `get_x`; a reader is named for what it returns |
| `is-prefix-nonboolean` | `is_` on something the binding does not return a bool from |
| `abbreviation` | `str`, `cfg`, `buf`, `idx`, `pos` where a user reads them |
| `module-plural` | a plural module that is not a keyed store of many things |
| `schema-vocabulary` | a component schema departing from the closed set — the discriminant is always `kind`, the meta key is always `type` |
| `module-undocumented`, undocumented function | anything a script can call that the generated reference could not describe |

Every exemption carries its reason inline, so it stops being cited as
precedent for the next one.

## Determinism is a gate, not a test

The promise is that identical inputs produce bit-for-bit identical simulation
on every platform, so the two ways it breaks — wall-clock time and entropy
reaching gameplay — are compile errors.

- `clippy.toml` disallows `Instant::now`, `SystemTime::now`, `rand::random`
  and `RandomState`. A call genuinely outside the simulation carries
  `#[allow(clippy::disallowed_methods, reason = "..")]` saying which it is,
  and `allow-without-reason` makes the reason mandatory.
- `platform-float-math` and `nondeterministic-iteration` catch what clippy
  cannot see.
- `.gitattributes` pins LF in the index and on every checkout: a CRLF checkout
  on Windows CI exports different pack bytes from the same sources.

Then CI checks it four ways:

1. **Two exports agree.** `scripts/e2e.sh` exports every example twice, in two
   separate processes, and the packs must be byte-identical. A pack was once
   written in hash order — stable within a process, different in the next.
2. **Every platform exports the same bytes.** Each platform uploads its pack
   digests; `scripts/compare_packs.sh` diffs them.
3. **Every platform steps the same simulation.** Each platform records one
   digest per tick for 600 frames over five example projects;
   `scripts/determinism_trace.sh compare` diffs the traces, and the first
   differing line is the tick they parted on.
4. **The same sources build the same binary.**
   `scripts/reproducible_build.sh` builds twice on one runner and compares. If
   the binary is not reproducible, "same binary, same simulation" is not
   testable and a divergence cannot be bisected.

A change that alters a recorded digest has to say why.

## Tests

Roughly 1,200 `#[test]` functions across 22 crates, with 114 integration test
files, run on all three desktop platforms. Beyond `cargo test --workspace`:

- `cargo test -p balaur_plugin --features dylib` and
  `cargo test -p balaur --features extensions` — the dlopen path, the cdylib,
  and the tests that load one at run time.
- `cargo build -p balaur_cli --no-default-features`, plus core and physics
  tested without them. A build with a subsystem switched off is the point of
  having features, and nothing else exercises one.
- `scripts/e2e_tests.sh` — suites where a full app boots over real sockets
  (`balaur_http`, `balaur_websocket`, `balaur_gamend`, `balaur_platform`).
  They gate on `BALAUR_E2E` so a plain local `cargo test` stays fast; CI always
  sets it.

A test's name is a sentence about behaviour — `freeing_a_node_frees_its_children`,
not `test_free`. A test that asserts from inside a script carries one control
proving the script ran at all, or its assertions hold vacuously when it does
not.

## End to end, over every example

`scripts/e2e.sh` runs each of the six example projects twenty ways, on each of
the three platforms:

- **run** — dev mode, straight from the sources.
- **export**, twice — the two packs must be identical.
- **play** — the exported pack, with no sources and no compiler present.
- **edit**, sixteen times — the editor is itself a Balaur project, so it is
  booted headless against every example: the scene it opens, then `undo`,
  `layout`, `rig`, `polygon`, `showcase`, `plugin`, `clipboard`, `assets`,
  `picking`, `props`, `instances`, `placing`, `timings`, `session` and
  `theme`, each a scripted state the editor drives itself through.

Every pass is held to two bars: **a clean exit and a clean log.** A logged
`ERROR` fails the run, and so does the editor's own
`did not resolve in the mirror` — an invariant it states at WARN, because the
document and the mirror are built from the same TOML. When that check broke,
nested nodes silently had no ref, so no inspector, no gizmo and no transform
read, and nothing else failed.

Headless covers loading, mirroring, node resolution and asset rebinding — not
drawing, which needs a window. `scripts/uiaudit.sh` covers drawing: one PNG
per editor screen, rendered offscreen, catalogued in `docs/EDITOR-SCREENS.md`.
Regenerate and diff the pair before reviewing a shell change. Offscreen means
a frame really is drawn, which is where the layout assertions can run.

## Documentation cannot drift

Hand-written docs go stale silently, so the ones that can be derived are.

- `cargo doc --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` —
  mostly about intra-doc links, where a renamed item leaves a dead link.
- `scripts/gen_docs.py --check` regenerates `docs/generated/` from cargo
  metadata and a booted engine, and fails on any diff.
- `api_lints.py` requires a doc line on every script module and every function
  in it — the generated reference is only as good as what the modules declare,
  and a function renamed without its entry loses its line silently.
- `scripts/third_party_notices.py --check`. Licence checking is off in
  `deny.toml`, so `THIRD-PARTY-NOTICES.md` is the only record of what a built
  binary combines, and it must not go stale.

The website builds its reference from the same `api.json`, so the published
docs cannot describe an engine that does not exist.

## Performance

Feature tests and performance tests stay apart. Budgets live in
`crates/balaur_bench/tests/budgets.rs` and assert orders of magnitude, never
percentages: a CI runner is shared and throttled, and a gate that cries wolf
gets ignored, which is worse than no gate. Every ceiling is ten times a
measured run and is written down once, in `budgets.toml`, by
`scripts/bench.py --record`. They catch an accidental O(n²), a lock held
across a frame, or a compile moved into the hot path.

`scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a real run on a
real machine, case for case against Godot with Rapier, Box2D v3 and Jolt.

## Supply chain and releases

- `cargo-deny` on every push: advisories, bans and sources, with
  `all-features` so the audit reaches the windowed dependency tree a headless
  build never compiles. Yanked crates are denied; unknown registries and git
  sources are denied, with two pinned forks allowlisted by name and reason.
- Releases carry `SHA256SUMS` and build-provenance attestations, verifiable
  with `gh attestation verify`.
- `build.yml` exports a game using the same published actions a player's own
  repository calls — `setup`, `export-game`, `build-engine` — against the
  engine that run just built. An action that stopped working fails here,
  rather than on the day someone else runs it.

## What review is left to do

Everything above is mechanical, which is the point: review spends its
attention on the parts a script cannot check — whether the change serves the
principles, whether the name is the right name, whether the test asserts on
behaviour a caller can see. One concern per pull request; code, tests, docs
and a `CHANGELOG.md` line land together.
