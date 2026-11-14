# Quality

Every check that holds the engine, and what enforces it. `CONTRIBUTING.md`
says how to run them locally and what each costs.

## CI

`runner.yml` is the one push and pull-request trigger, and calls the rest.

| Workflow | Covers |
| --- | --- |
| `lint.yml` | format, every clippy shape, the house, comment, string, prose and API lints, the notices, cargo-deny |
| `docs.yml` | rustdoc, and `docs/generated/` against a booted engine |
| `test.yml` | the tests, the socket suites, the determinism traces, the reproducible build, coverage, and the compiler's fuzz targets |
| `build.yml` | every platform's build, templates, exports and signing check |
| `container.yml`, `container-signer.yml` | the engine's and the signing toolchain's container images (`docker/README.md`) |

## The compiler

`rust-toolchain.toml` pins the toolchain, with `rustfmt` and `clippy`, so every
machine runs one linter version. `cargo fmt --all --check`, then
`cargo clippy --all-targets -- -D warnings` over each shape:

- the workspace, then once each for `extensions` and `apple`
- the templates `scripts/package.sh` builds: `2d`, `3d` and `bare` with a
  window, and `server` without one
- `wasm32-unknown-unknown` twice: the web runtime's features, and the defaults
- `aarch64-linux-android` and `aarch64-apple-ios` over the workspace, then the
  iOS template's `apple`: the code no host build compiles.
  `scripts/precommit.sh` skips either without its target, and Android without
  the NDK, whose clang builds the C and C++ under `ring` and HarfBuzz.
- `examples/extension_greeter`, outside the workspace on purpose: the only
  thing proving an extension builds without the engine's build tree.

## House rules

`scripts/house_lints.py` walks every `.rs`, every `.bs` for the three size
rules, and reads `scripts/showcase.sh` and `scripts/features.sh`. **ERROR**
fails CI; **REPORT** prints only.

| Rule | Fails on |
| --- | --- |
| `platform-float-math` | `.sin()`, `f32::sin(x)`, `.powf()` and the rest of the inexact list |
| `nondeterministic-iteration` | iterating a `HashMap`/`HashSet`; `DetHashMap`/`DetHashSet` are the substitutes |
| `channel-outside-external-io` | an `mpsc` channel in a file naming neither `ExternalIo` nor `replay::suppressed` |
| `world-write` | `.world_mut()` outside `balaur_core` and tests; `Engine`'s spawn, insert, remove, reparent and despawn are the door |
| `borrow-across-reentry` | a `RefCell` borrow still held across a call that can re-enter the engine |
| `epaint-outside-paint` | egui's painter, a `Shape` or `epaint` named in `balaur_ui` outside `widget/paint.rs` and the pass in `lib.rs` |
| `allow-without-reason` | `#[allow(..)]` with no `reason = ".."` and no comment |
| `unjustified-unwrap` | `unwrap`/`expect` outside tests with no justification and no descriptive message |
| `timed-wait` | a `thread::sleep`, a socket timeout, a `recv_timeout`, `wait_timeout` or `park_timeout`, or a `tokio::time::sleep` or `interval`, outside tests with no comment saying why (`AGENTS.md`, "Threads") |
| `log-instead-of-tracing` | `log::*` in our own code; its records carry no fields to filter on |
| `todo-without-issue` | a TODO or FIXME with no issue |
| `fn-too-long`, `file-too-long` | a function over 120 lines, a file over 1200, in `.rs` and `.bs` |
| `fn-too-many-branches` | a function over 20 written branches, in `.rs` and `.bs`: each `if`, `match`, `for`, `while` and `let`-`else`. Clippy's `cognitive_complexity` stays off, since it counts what `assert_eq!` expands to |
| `comment-too-long` | a plain-comment block over twelve lines |
| `unit-test-target` | a target that holds no unit tests and does not say `test = false`, or one that says it and holds some |
| `showcase-milestone` | a picture `scripts/showcase.sh` takes and files under no milestone, or a milestone naming a take that is gone |
| `vocabulary-literal` | a params key spelled at a call site in a crate that keeps a `vocabulary.rs` |
| `det-prefix-misuse`, `dimension-casing`, `dimension-snake`, `install-verb`, `system-verb`, `engine-param-name`, `resource-suffix`, `new-resource-type`, `fn-suffix-on-struct`, `pub-inner`, `component-registration-doc` | the Rust names in `CONTRIBUTING.md` |
| `std-fs`, `is-absolute` | `std::fs` and `Path::is_absolute` in engine crates, which the web build has no disk for; `files::backend` and `files::rooted` are the substitutes |
| `setting-unit` | a setting or option that measures time, rate or distance and names no unit (`NAMING.md` N21) |
| `theme-token`, `theme-key` | a colour token that names a hue or a family suffix outside the set, and a theme key that is no widget property (N18, N19) |
| `template-features` | `balaur_android`'s default features differing from `GAME_FEATURES` in `scripts/features.sh`, the set every game template is built with |

## Comments

`scripts/comment_lints.py` covers `.rs`, `.bs`, `.py`, `.sh`, `.yml` and
`.toml`. Generated files skip themselves by their banner.

| Rule | Fails on |
| --- | --- |
| `essay` | more than three consecutive plain-comment lines; a shell script's leading block gets eight |
| `restates-code` | a comment whose words add nothing to the line below |
| `banner` | a divider; the code's own structure is the divider |
| `commented-out-code` | code in a comment; git remembers it |
| `test-doc` | a `///` on a `#[test]`: the name says what it checks, a plain `//` says why |
| `doc-restates-signature` | a `///` on an item, in `.rs` or `.bs`, whose words are 70% its signature's |
| `doc-essay` (report) | more than three doc lines on an item that is not `pub`; fields and variants are left out, since a component's become its schema |

## Strings

`scripts/string_lints.py` covers shipped `.rs` and `.bs`, by crate or script
package. It leaves out tests, the editor's `selftest.bs` and `showcase.bs`,
generated files and `const` items. It also leaves out a callback's method name
(`on_click: "handle"`, `.call("step")`), a reader's text (`text: "reset"`),
and what must stay a literal: a class field's default, `cfg`, `quote!`,
`toml!`, `json!`, a file's own `macro_rules!`. A word is lower-case, snake or kebab, two
characters or more. `--project <path>` lints a game's own scripts, without its
`addons/`, which answer where they are written.

| Rule | Fails on |
| --- | --- |
| `constant-respelled` | a literal equal to a constant of its own name in the file, to an engine constant holding an underscore, to a `kind:` widget constant, or to a package constant holding an underscore |
| `word-compared` | a word on one side of `==` or `!=`, or a match arm |
| `word-repeated` | a word spelled twice in one file or three times in one package |

Every finding fails. `--summary` prints findings per package with its
most-spelled words.

## The script API

`scripts/api_lints.py` checks the names in `docs/NAMING.md` against what a
**booted engine** reports through `balaur api`: derived constants like
`input.KEY_SPACE` exist only at registration time, and a name scripts cannot
reach is not API. CI and `precommit.sh` read the committed
`docs/generated/api.json`; `gen_docs.py --check` boots the engine and fails if
that file is stale.

| Check | Fails on |
| --- | --- |
| `getter-prefix` | a reader named `get_x` |
| `is-prefix-nonboolean` | `is_` on a non-boolean |
| `abbreviation` | `str`, `cfg`, `buf`, `idx`, `pos`, or an `h`+noun like `hslider`, where a user reads them |
| `module-plural` | a plural module that is not a keyed store |
| `schema-vocabulary` | a schema departing from the closed set: the discriminant is `kind`, the meta key is `type` |
| `any` | a function or field typed `any` with no reviewed reason (N25) |
| `hook-name` | a hook the engine tells that does not start with `on_`, or one told a flag's new value that does not end in `_changed` (N22) |
| `module-undocumented`, `function-undocumented`, `component-undocumented`, `asset-undocumented` | anything the generated reference could not describe |
| `describes-nothing` | a doc entry for a function no longer registered |
| `acts-on-unknown` | a function documented as acting on a component nobody registers |
| `handle-method-property` | a handle method spelled as a property of its component (N26) |
| `handle-method-twice` | two functions one handle would call by the same name (N26) |
| `setter-without-reader` (report) | a `set_x` with no `x` reader and no comment saying why |

Every exemption carries its reason inline, so it stops being cited as precedent.

## Documentation

- `cargo doc --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings`:
  mostly intra-doc links, where a rename leaves a dead one.
- `scripts/gen_docs.py --check` regenerates `docs/generated/` from cargo
  metadata and a booted engine, and fails on any diff. The site builds its
  reference from the same `api.json`.
- `scripts/prose_lints.py` fails a roadmap row over one sentence or 25 words,
  and every hand-written `.md` on the mechanical half of the `avoid-ai-writing`
  skill. On `docs/ROADMAP.md` it reports sentence length, filler and em dashes
  without failing.
- `scripts/third_party_notices.py --check`. Licence checking is off in
  `deny.toml`, so `THIRD-PARTY-NOTICES.md` is the only record of what a built
  binary combines.

## Determinism

- `clippy.toml` disallows `Instant::now`, `SystemTime::now`, `rand::random` and
  `RandomState`. A call outside the simulation carries
  `#[allow(clippy::disallowed_methods, reason = "..")]`.
- `platform-float-math` and `nondeterministic-iteration` catch what clippy
  cannot see.
- `.gitattributes` pins LF: a CRLF checkout on Windows exports different pack
  bytes from the same sources.
- **Every platform steps the same simulation.** `scripts/determinism_trace.sh`
  records one digest per tick for 600 frames over five examples on Linux,
  macOS and Windows, and `compare` diffs them.
- **The same sources build the same binary.** `scripts/reproducible_build.sh`
  builds twice and compares, so a divergence can be bisected.
- `scripts/e2e.sh` exports every example twice in separate processes, and the
  packs must be byte-identical.

A change that alters a recorded digest says why.

## Tests

`cargo test --workspace` runs on all three desktop platforms. Beyond it:

- `cargo test -p balaur_plugin --features extensions` and `-p balaur --features
  extensions`: the dlopen path, the cdylib, and loading one at run time.
- `cargo build -p balaur_cli --no-default-features`, with core and physics
  tested without them: nothing else exercises a subsystem switched off.
- `cargo test -p balaur_render --features aseprite`, which `precommit.sh` runs
  and `test.yml` does not.
- **The Balaur Script corpus**: `crates/balaur_bs/tests/programs/`, `.bs` files
  whose `// expect:` lines are calls and their answers and whose `// error:`
  lines are their compile errors, written out whole. Each runs on every engine
  the build has (wasmi, wasmtime, native code through C), which must agree to
  the bit; `cargo test --workspace` turns all three on. CI also runs it on the
  browser's engine in Node (`--test web`, wasm32). `BLESS=1` rewrites the
  answers from what ran.
- **The language chapter**: every ```` ```bs ```` fence in `docs/language/`
  says what it does in its comments. `// f(x) == v` runs the call,
  `// f(x) traps` expects a trap, and `// error: L:C: message` is the compile
  errors written out whole. A first line `// util.bs` names a module other
  fences on the page `use`. A fence with no item at column 0 compiles inside a
  function. `crates/balaur_bs/tests/suite/chapter.rs` holds it to that, and
  `crates/balaur/tests/suite/chapter.rs` compiles the fences that use the
  engine against its real API, warnings included. It lives here so a language
  change and its pages land in one commit.
- **Broken programs**: `tests/suite/mutants.rs` cuts, splices and seeds tokens
  into every corpus program, then reads, formats, analyses, compiles and runs
  each under fuel. Nothing may panic, and every module written validates.
  `BS_FUZZ_SEED` and `BS_FUZZ_MUTANTS` (12 a program by default) widen a run.
  `crates/balaur_bs/fuzz/` holds the same two checks as `cargo fuzz` targets,
  `compile` and `run`, which `test.yml` runs for twenty minutes each.
- **Random programs**: `tests/suite/random.rs` generates well-typed programs
  and runs each on every engine and on a small interpreter kept for the test,
  which must agree. The engines share one compiler, so only this catches it
  writing the wrong code. 60 a push; `BS_RANDOM_PROGRAMS` sets more. A
  disagreement is shrunk, printed and saved under `tests/programs/random/`.
- `scripts/check_scripts.sh`: `balaur check --strict` over the editor, its
  library and templates, and every example, on Linux after the tests.
- `scripts/e2e_tests.sh`: suites where a full app boots over real sockets
  (`balaur_http`, `balaur_websocket`, `balaur_platform`, and the library's
  Gamend addon in `balaur`), gated on `BALAUR_E2E`. The Gamend ones talk to a
  local Gamend on port 4000 (`GAMEND_URL` overrides): CI starts one with
  Gamend's `setup-gamend` action, and locally `gamend daemon` serves one.

What keeps a run short:

- The editor and the tests build on one feature set, `cargo editor` and
  `cargo suite` (`.cargo/config.toml`), so an edit compiles each crate once
  for both; `-p balaur_cli` would compile every engine crate a second time.
- A target whose sources hold no unit tests says `test = false`
  (`unit-test-target`), so no empty test binary is linked.
- A dev build carries no debug info, so no link goes through `dsymutil`;
  `--profile debugging` keeps it for a debugger.
- A long test does its work on every core, and nextest starts it first
  (`.config/nextest.toml`).
- A dependency's heavy generic is called through a concrete type the
  dependency compiles itself at opt 3 (`image`'s `DynamicImage`), not
  instantiated in a crate built at opt 0.

## End to end, over every example

`scripts/e2e.sh` runs each of the sixteen examples these ways. CI does not run
it; `scripts/precommit.sh --e2e` does.

- **check**: every script a scene attaches, compiled, plus the handle calls no
  compiler can see, and each handler, listener and row naming an event nothing
  announces. Run `--strict`, so a warning fails. The editor, its library and
  each template are checked first, each from its own root.
- **run**: dev mode from sources.
- **export**, twice: the packs must be identical.
- **play**: the export pack, no sources, no compiler.
- **edit**: the editor booted headless against every example. The per-project
  self-test states (`EXAMPLE_STATES`) run on `angrynerds`, `hello`, `objects`
  and `rig`, which between them hold what every state needs. The editor's own
  states run once.
- **render**, twice: the game and the editor surface-less on a real GPU, where
  a texture the backend cannot bind or a shader it rejects warns. The step
  reads the `rendering ... offscreen` line back, so a binary built with no
  `window` feature fails.

Two bars: **a clean exit and a clean log.** A logged `ERROR` or `WARN` fails,
bar a warning a self-test names first with `expect_warning`.
`scripts/web_smoke.mjs` holds a web pack to the same log in a browser.

`BALAUR_E2E_KEEP_GOING=1` lists a failure and goes on, so one broken example
does not hide the ones after it. A game checked out beside the engine is
checked and run too: `../polyglot-pirates-app`, or the folders
`BALAUR_E2E_PROJECTS` names. Every run keeps its saves, logs and editor
preferences under the output folder, through `BALAUR_DATA_DIR`.

Three more runs look where a log does not, and none runs on a push:

- `scripts/monkey.sh` feeds the editor seeded random pointer moves, clicks,
  drags, scrolls and keys from `scripts/plugins/monkey.bs`, and fails on a
  crash, a hang, an `ERROR` or a `WARN`. Fed input reaches what a script
  reads, the viewport and the shortcuts; a widget hears the window's own
  events, so it presses no button.
- `scripts/soak.sh` runs each example headless for 600 frames and for 12,000,
  and fails one whose peak memory grew by more than 32 MB between them.
- `scripts/pictures.py` runs each example offscreen at a fixed tick for 200
  frames and compares the last one with the PNG in `scripts/pictures/`. It
  fails when over 0.5% of the pixels moved, and `--update` writes the set,
  which is the release Mac's GPU's.

What a screen looks like is `scripts/uiaudit.sh`: one PNG per editor screen,
offscreen, catalogued in `docs/EDITOR-SCREENS.md`. Regenerate and diff before
reviewing a shell change.

## Performance

Readings live in `crates/balaur_bench/budgets.toml`, one per benchmark, written
by `scripts/bench.py --record` with a `[machine]` table naming the CPU, core
count, OS, toolchain and commit they came off.

`scripts/bench.py --compare`, on the machine that recorded them, prints every
benchmark's shift, worst first, and fails on anything a fifth slower.
`scripts/bench.py --check` is the loose one, against a ceiling ten times each
reading, for any other machine. Nothing in CI gates on either: a shared,
throttled runner times a benchmark badly.

CI gates on counts instead. `crates/balaur_bs/tests/suite/budgets.rs` holds
each script operation to the fuel wasmi charges for it and the host calls it
makes, the same on every machine.

`scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a real run, case
for case against Godot with Rapier, Box2D v3 and Jolt.

`scripts/bench_load.py` measures what one kind of node, or one shape of script,
costs a frame. Each case is N of one thing under `examples/benchmark`, built
once and measured as it sits, and the table splits the frame into the script,
the mirror, the UI pass and the renderer's two halves. It reports the
statements a node's script ran beside the milliseconds, since that count is
the same on every machine. A case is one entry in `cases_kinds.bs` or
`cases_scripts.bs`; it runs on demand, never in CI.

## Supply chain and releases

- `cargo-deny` on every push: advisories, bans and sources, with
  `all-features` so the audit reaches the windowed tree. Yanked crates, unknown
  registries and git sources are denied, bar the three pinned forks
  `deny.toml` allows by name.
- Releases carry `SHA256SUMS` and build-provenance attestations, verifiable
  with `gh attestation verify`.
- `build.yml` builds the engine and exports a game with the published actions
  a player's own repository calls, `build-engine` and `export-game`, so a
  broken action fails here first.
- `build.yml` also exports and runs a new project on each packaged template,
  and runs `scripts/signing_check.sh` (`BALAUR_SIGNING_CHECK`) and
  `scripts/export_check.sh` per platform. It runs `scripts/web_smoke.mjs` over
  every web export pack through `scripts/package_play.sh`, which boots each
  example on the web runtime its `[export] runtime` picked.
- `build.yml` builds a `-2d`, `-3d`, `-bare` and `-server` template of every
  desktop, exports onto each an example that fits only it, and plays 60 frames
  (`VARIANT=... scripts/package.sh`); the mobile and web variants go through
  `scripts/export_check.sh`.
- `scripts/check_web_module.sh` holds both web stagings to what `balaur.js`
  names beside it, wasm-bindgen's `snippets/` included: a static import that
  404s stops the module evaluating while the build stays green.

## What review is left

Everything above is mechanical. Review spends its attention on what a script
cannot check: whether the change serves the principles, whether the name is the
right name, whether the test asserts on behaviour a caller can see.
