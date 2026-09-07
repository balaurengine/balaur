# Quality

Every rule is a script that fails, and every script runs on every push and pull
request. `AGENTS.md` and `docs/NAMING.md` say what the rules are; this says what
enforces them.

## One entry point

`.github/workflows/runner.yml` owns the triggers — a push to `main`, a `v*` tag,
every pull request — and calls four reusable workflows, so a red X names itself.

| Workflow | Covers |
| --- | --- |
| `lint.yml` | everything that reads the code without running it |
| `docs.yml` | both kinds of documentation |
| `test.yml` | everything that runs the engine |
| `build.yml` | everything that produces a download |

`build.yml` runs on pull requests too, minus its publishing job.

## Before you push

```bash
scripts/precommit.sh
```

Everything below that one machine can run, in three streams, each feature shape
in its own target directory. `AGENTS.md` holds the tiers and what each covers.
Install the lints as a pre-push hook: `git config core.hooksPath .githooks`.

## The compiler, per platform and per feature

The toolchain is pinned in `rust-toolchain.toml` to the dependency tree's MSRV,
with `rustfmt` and `clippy`, so every machine runs one linter version.

- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets -- -D warnings` on Linux, macOS and
  Windows — `#[cfg(windows)]` code compiles nowhere else.
- Once per default-off feature, since code behind one is not compiled at all:
  `window` (kiss3d, wgpu, egui, the macOS dock icon), `extensions` (dlopen and
  the cdylib), `apple` (Game Center, StoreKit, objc2; macOS only).
- Once for `wasm32-unknown-unknown` with the web template's own flags: nothing
  else compiles `#[cfg(target_family = "wasm")]`, so a browser-only mistake
  used to reach CI as a failed download.
- `examples/extension_greeter`, deliberately outside the workspace: the only
  thing proving an extension builds without the engine's build tree.

## House rules no compiler enforces

`scripts/house_lints.py` walks every `.rs` and `.rn`. **ERROR** fails CI and is
mechanical; **REPORT** prints only — failing a build on a heuristic teaches
people to game the heuristic.

| Rule | Fails on |
| --- | --- |
| `platform-float-math` | `.sin()`, `f32::sin(x)`, `.powf()` and the rest of the inexact list; `sqrt`, `abs`, `floor` and friends are IEEE-exact and deliberately absent |
| `nondeterministic-iteration` | iterating a `HashMap`/`HashSet`; `DetHashMap`/`DetHashSet` are the substitutes |
| `channel-outside-external-io` | an `mpsc` channel in a file naming neither `ExternalIo` nor `replay::suppressed` |
| `allow-without-reason` | `#[allow(..)]` with no `reason = ".."` and no comment |
| `unjustified-unwrap` | `unwrap`/`expect` outside tests with no justification and no descriptive message |
| `log-instead-of-tracing` | `log::*` in our own code; its records carry no fields to filter on |
| `todo-without-issue` | a TODO or FIXME with no issue |
| `fn-too-long`, `file-too-long` | 120 lines, 1200 lines |
| `comment-too-long`, `comment-restates-name` | comment blocks, and comments that restate the line below |
| `det-prefix-misuse`, `dimension-casing`, `dimension-snake`, `install-verb`, `system-verb`, `engine-param-name`, `resource-suffix`, `new-resource-type`, `fn-suffix-on-struct`, `pub-inner`, `component-registration-doc` | the mechanical half of `docs/NAMING.md` |
| `rune-short-circuit`, `rune-rebound-let` | two Rune shapes that compile and then misbehave (`AGENTS.md`) |

- **The ratchet.** `scripts/house_lints_baseline.txt` records, per file and rule,
  how many violations predate the rule; anything above fails. Counts rather than
  line numbers, so it survives edits above them. `--debt` prints what is
  outstanding; deleting a line is progress, and nothing may be added by hand.
- **Adding a rule:** when the same bad pattern shows up twice, it becomes a lint.

## Comments

`scripts/comment_lints.py` covers `.rs`, `.rn`, `.py`, `.sh`, `.yml`, `.toml`.
Generated files skip themselves by their banner.

- **essay** — more than three consecutive plain-comment lines. A shell script's
  leading block gets eight; `house_lints.py` keeps a looser cap of twelve.
- **restates-code** — a comment whose words add nothing to the line below.
- Banners and dividers; the structure is the divider.

## Names

`docs/NAMING.md` holds sixteen rules, each tagged with a scope and the cost of
getting it wrong: `rust-internal` is compiler-caught, `script-api` breaks
projects at `balaur export`, `scene-file` breaks existing scenes and the
inspector generated from the same schemas.

`house_lints.py` covers the Rust half. `scripts/api_lints.py` covers the script
API by **booting the engine** and reading `balaur api` — derived constants like
`input.KEY_SPACE` exist only at registration time, and a name scripts cannot
reach is not API.

| Check | Fails on |
| --- | --- |
| `getter-prefix` | a reader named `get_x` |
| `is-prefix-nonboolean` | `is_` on a non-boolean |
| `abbreviation` | `str`, `cfg`, `buf`, `idx`, `pos` where a user reads them |
| `module-plural` | a plural module that is not a keyed store |
| `schema-vocabulary` | a schema departing from the closed set — the discriminant is `kind`, the meta key is `type` |
| `module-undocumented`, undocumented function | anything callable the generated reference could not describe |

Every exemption carries its reason inline, so it stops being cited as precedent.

## Determinism is a gate, not a test

- `clippy.toml` disallows `Instant::now`, `SystemTime::now`, `rand::random` and
  `RandomState`. A call genuinely outside the simulation carries
  `#[allow(clippy::disallowed_methods, reason = "..")]`.
- `platform-float-math` and `nondeterministic-iteration` catch what clippy
  cannot see.
- `.gitattributes` pins LF: a CRLF checkout on Windows CI exports different pack
  bytes from the same sources.

Then CI checks it four ways:

1. **Two exports agree.** `scripts/e2e.sh` exports every example twice in
   separate processes; the packs must be byte-identical. A pack was once written
   in hash order — stable within a process, different in the next.
2. **Every platform exports the same bytes** — `scripts/compare_packs.sh`.
3. **Every platform steps the same simulation** — one digest per tick for 600
   frames over five examples, diffed by `scripts/determinism_trace.sh compare`.
4. **The same sources build the same binary** —
   `scripts/reproducible_build.sh` builds twice and compares. Without it, "same
   binary, same simulation" is not testable and a divergence cannot be bisected.

A change that alters a recorded digest has to say why.

## Tests

1,509 `#[test]` functions across 22 crates, 135 integration files, on all three
desktop platforms. Beyond `cargo test --workspace`:

- `cargo test -p balaur_plugin --features dylib` and `-p balaur --features
  extensions` — the dlopen path, the cdylib, and loading one at run time.
- `cargo build -p balaur_cli --no-default-features`, plus core and physics
  tested without them: nothing else exercises a subsystem switched off.
- `scripts/e2e_tests.sh` — suites where a full app boots over real sockets
  (`balaur_http`, `balaur_websocket`, `balaur_gamend`, `balaur_platform`), gated
  on `BALAUR_E2E` so a local `cargo test` stays fast.

## End to end, over every example

`scripts/e2e.sh` runs each of the nine examples thirty-one ways, on three
platforms:

- **check** — every script a scene attaches, compiled. The cheapest gate, and
  the only one that names a file and a line rather than a symptom.
- **run** — dev mode from sources.
- **export**, twice — the packs must be identical.
- **play** — the exported pack, no sources, no compiler.
- **edit**, twenty-six times — the editor booted headless against every
  example: the scene it opens, then `undo`, `layout`, `rig`, `polygon`,
  `weights`, `bone map`, `physical bones`, `tiles`, `showcase`, `plugin`,
  `clipboard`, `script paths`, `assets`, `picking`, `props`, `instances`,
  `placing`, `timings`, `session`, `theme`, `selection`, `events`, `library`,
  `pen`, `drag-in`.

Two bars: **a clean exit and a clean log.** A logged `ERROR` fails, and so does
the editor's `did not resolve in the mirror` — an invariant it states at WARN,
since document and mirror are built from the same TOML. When that broke, nested
nodes silently had no ref, so no inspector, no gizmo, no transform read, and
nothing else failed.

Headless covers loading, mirroring, node resolution and asset rebinding, not
drawing. `scripts/uiaudit.sh` covers drawing: one PNG per editor screen,
offscreen, catalogued in `docs/EDITOR-SCREENS.md`. Regenerate and diff before
reviewing a shell change.

## Documentation cannot drift

- `cargo doc --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` —
  mostly intra-doc links, where a rename leaves a dead one.
- `scripts/gen_docs.py --check` regenerates `docs/generated/` from cargo
  metadata and a booted engine, and fails on any diff.
- `api_lints.py` requires a doc line on every script module and function.
- `scripts/third_party_notices.py --check`. Licence checking is off in
  `deny.toml`, so `THIRD-PARTY-NOTICES.md` is the only record of what a built
  binary combines.

The website builds its reference from the same `api.json`, so the published docs
cannot describe an engine that does not exist.

## Performance

Budgets live in `crates/balaur_bench/budgets.toml`, one ceiling per benchmark,
each ten times a measured run and written by `scripts/bench.py --record`.
`scripts/bench.py --check` reports against them. Nothing in CI gates on them: a
shared, throttled runner times a benchmark badly, and a gate that cries wolf
gets ignored. Read them when a change should have moved a number.

`scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a real run, case for
case against Godot with Rapier, Box2D v3 and Jolt.

## Supply chain and releases

- `cargo-deny` on every push: advisories, bans and sources, with `all-features`
  so the audit reaches the windowed tree. Yanked crates, unknown registries and
  git sources are denied, with two pinned forks allowlisted by name and reason.
- Releases carry `SHA256SUMS` and build-provenance attestations, verifiable with
  `gh attestation verify`.
- `build.yml` exports a game with the same published actions a player's own
  repository calls — `setup`, `export-game`, `build-engine` — so a broken action
  fails here rather than on the day someone else runs it.

## What review is left

Everything above is mechanical, so review spends its attention on what a script
cannot check: whether the change serves the principles, whether the name is the
right name, whether the test asserts on behaviour a caller can see.
