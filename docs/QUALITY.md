# Quality

Tools and processes for ensuring the code quality remains high.

## CI/CD

| Workflow | Covers |
| --- | --- |
| `runner.yml` | the one push and pull-request trigger; calls the four below |
| `lint.yml` | runs format, clippy, house rules and supply chain checks |
| `docs.yml` | checks if the documentation is generated correctly |
| `test.yml` | runs the tests, and the Balaur Script compiler's fuzz targets |
| `build.yml` | builds and exports |

## Before you push

```bash
scripts/precommit.sh
```

## The compiler

The toolchain is pinned in `rust-toolchain.toml`,
with `rustfmt` and `clippy`, so every machine runs one linter version.

- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`
- Once for `extensions`, `apple`
- The game templates `scripts/package.sh` builds: `2d` and `3d`, one physics
  world each with a window, and `server` without one
- Twice for `wasm32-unknown-unknown`: the web runtime's features, and the defaults
- The workspace for `aarch64-linux-android` and `aarch64-apple-ios`, then the
  iOS template's `apple`: the code no host build compiles.
  `scripts/precommit.sh` skips either without its target, and Android without
  the NDK, whose clang builds the C and C++ under `ring` and HarfBuzz.
- `examples/extension_greeter`, deliberately outside the workspace: the only
  thing proving an extension builds without the engine's build tree.

## House rules no compiler enforces

`scripts/house_lints.py` walks every `.rs` and `.rn`, and reads
`scripts/showcase.sh` and `scripts/features.sh`. **ERROR** fails CI and is
mechanical; **REPORT** prints only.

| Rule | Fails on |
| --- | --- |
| `platform-float-math` | `.sin()`, `f32::sin(x)`, `.powf()` and the rest of the inexact list |
| `epaint-outside-paint` | egui's painter, a `Shape` or `epaint` named in `balaur_ui` outside `widget/paint.rs` and the pass in `lib.rs` |
| `nondeterministic-iteration` | iterating a `HashMap`/`HashSet`; `DetHashMap`/`DetHashSet` are the substitutes |
| `channel-outside-external-io` | an `mpsc` channel in a file naming neither `ExternalIo` nor `replay::suppressed` |
| `allow-without-reason` | `#[allow(..)]` with no `reason = ".."` and no comment |
| `unjustified-unwrap` | `unwrap`/`expect` outside tests with no justification and no descriptive message |
| `timed-wait` | a `thread::sleep`, a socket read or write timeout, a `recv_timeout`, `wait_timeout` or `park_timeout`, or a `tokio::time::sleep` or `interval`, outside tests with no comment saying why: a thread sleeps on its event, `docs/PLAN-io.md` |
| `log-instead-of-tracing` | `log::*` in our own code; its records carry no fields to filter on |
| `todo-without-issue` | a TODO or FIXME with no issue |
| `fn-too-long`, `file-too-long` | 120 lines, 1200 lines |
| `comment-too-long`, `comment-restates-name` | comment blocks, and comments that restate the line below |
| `showcase-milestone` | a picture `scripts/showcase.sh` takes and files under no milestone, or a milestone naming a take that is gone |
| `det-prefix-misuse`, `dimension-casing`, `dimension-snake`, `install-verb`, `system-verb`, `engine-param-name`, `resource-suffix`, `new-resource-type`, `fn-suffix-on-struct`, `pub-inner`, `component-registration-doc` | the mechanical half of `docs/NAMING.md` |
| `std-fs`, `is-absolute` | `std::fs` and `Path::is_absolute` in engine crates, which the web build has no disk for; `files::backend` and `files::rooted` are the substitutes |
| `hover-only-control` | an editor control shown only while hovered, which a finger cannot reach |
| `setting-unit` | a setting or option that measures time, rate or distance and names no unit (`NAMING.md` N21) |
| `theme-token`, `theme-key` | a colour token that names a hue or a family suffix outside the set, and a theme key that is no widget property (N18, N19) |
| `template-features` | `balaur_android`'s default features differing from `GAME_FEATURES` in `scripts/features.sh`, the set every game template is built with |

## Comments

`scripts/comment_lints.py` covers `.rs`, `.rn`, `.py`, `.sh`, `.yml`, `.toml`.
Generated files skip themselves by their banner.

- **essay** — more than three consecutive plain-comment lines. A shell script's
  leading block gets eight; `house_lints.py` keeps a looser cap of twelve.
- **restates-code** — a comment whose words add nothing to the line below.
- Banners and dividers; the structure is the divider.

## Strings

`scripts/string_lints.py` covers shipped `.rs` and `.bs`, by crate or script
package. Tests, the editor's `selftest.bs` and `showcase.bs`, generated files,
`const` items, a callback's method name (`on_click: "handle"`, `.call("step")`)
a reader's text (`text: "reset"`) and what must stay a literal (a class field's
default, `cfg`, `quote!`, `toml!`, `json!`, a file's own `macro_rules!`) are left out. A word is lower-case, snake or kebab, two
characters or more. `--project <path>` lints a game's own scripts, without its
`addons/`, which answer where they are written.

| Rule | Fails on |
| --- | --- |
| `constant-respelled` | a literal equal to a constant of its own name in the file, to an engine constant holding an underscore, to a `kind:` widget constant, or to a package constant holding an underscore |
| `word-compared` | a word on one side of `==` or `!=`, or a match arm |
| `word-repeated` | a word spelled twice in one file or three times in one package |

Every finding fails. `--summary` prints findings per package with its
most-spelled words.

## Names

`docs/NAMING.md` holds sixteen rules, each tagged with a scope and the cost of
getting it wrong: `rust-internal` is compiler-caught, `script-api` breaks
projects at `balaur export`, `scene-file` breaks existing scenes and the
inspector generated from the same schemas.

`house_lints.py` covers the Rust half. `scripts/api_lints.py` covers the script
API from what a **booted engine** reports through `balaur api` — derived
constants like `input.KEY_SPACE` exist only at registration time, and a name
scripts cannot reach is not API. CI and `precommit.sh` read the committed
`docs/generated/api.json`; `gen_docs.py --check` is what boots the engine and
fails if that file is stale.

| Check | Fails on |
| --- | --- |
| `getter-prefix` | a reader named `get_x` |
| `is-prefix-nonboolean` | `is_` on a non-boolean |
| `abbreviation` | `str`, `cfg`, `buf`, `idx`, `pos` where a user reads them |
| `module-plural` | a plural module that is not a keyed store |
| `schema-vocabulary` | a schema departing from the closed set — the discriminant is `kind`, the meta key is `type` |
| `vocabulary-literal` | a params key spelled at a call site in a crate that keeps a `vocabulary.rs` |
| `module-undocumented`, `function-undocumented`, `component-undocumented`, `asset-undocumented` | anything the generated reference could not describe |
| `describes-nothing` | a doc entry for a function no longer registered |
| `acts-on-unknown` | a function documented as acting on a component nobody registers |
| `handle-method-property` | a handle method spelled as a property of its component (N26) |
| `handle-method-twice` | two functions one handle would call by the same name (N26) |
| `setter-without-reader` (report) | a `set_x` with no `x` reader and no comment saying why |

Every exemption carries its reason inline, so it stops being cited as precedent.

## Determinism is a gate, not a test

- `clippy.toml` disallows `Instant::now`, `SystemTime::now`, `rand::random` and
  `RandomState`. A call genuinely outside the simulation carries
  `#[allow(clippy::disallowed_methods, reason = "..")]`.
- `platform-float-math` and `nondeterministic-iteration` catch what clippy
  cannot see.
- `.gitattributes` pins LF: a CRLF checkout on Windows CI exports different export pack
  bytes from the same sources.

Then CI checks it two ways:

1. **Every platform steps the same simulation** — one digest per tick for 600
   frames over five examples, diffed by `scripts/determinism_trace.sh compare`.
2. **The same sources build the same binary** —
   `scripts/reproducible_build.sh` builds twice and compares. Without it, "same
   binary, same simulation" is not testable and a divergence cannot be bisected.

Locally, `scripts/e2e.sh` exports every example twice in separate processes;
the export packs must be byte-identical. An export pack was once written in hash order, stable
within a process and different in the next.

A change that alters a recorded digest has to say why.

## Tests

About 2,040 `#[test]` functions across 22 crates, 159 integration files, on all
three desktop platforms. Beyond `cargo test --workspace`:

- `cargo test -p balaur_plugin --features extensions` and `-p balaur --features
  extensions` — the dlopen path, the cdylib, and loading one at run time.
- `cargo build -p balaur_cli --no-default-features`, plus core and physics
  tested without them: nothing else exercises a subsystem switched off.
- `cargo test -p balaur_render --features aseprite`, locally only:
  `precommit.sh` runs it and `test.yml` does not.
- **The Balaur Script corpus** — `crates/balaur_bs/tests/programs/`, `.bs`
  files whose `// expect:` lines are calls and their answers and whose
  `// error:` lines are their compile errors, written out whole. Each runs on
  every engine the build has (wasmi, wasmtime, native code through C), which
  must agree to the bit; `cargo test --workspace` turns all three on. CI
  also runs it on the browser's engine in Node (`--test web`, wasm32).
  `BLESS=1` rewrites the answers from what ran.
- **The language chapter** — `docs/language/`, which forge's
  `mix forge.site.sync` copies to the site. Every ```` ```bs ```` fence says what
  it does in its comments: `// f(x) == v` runs the call, `// f(x) traps` expects
  a trap, `// error: L:C: message` is the compile errors written out whole, and
  a first line `// util.bs` names a module other fences on the page `use`. A
  fence with no item at column 0 compiles inside a function.
  `tests/suite/chapter.rs` holds it to that;
  `crates/balaur/tests/suite/chapter.rs` compiles the fences that use the
  engine against its real API, warnings included. It lives here so a language
  change and its pages land in one commit, and so CI can read it: forge is a
  private repository with no workflows.
- **Broken programs** — `tests/suite/mutants.rs` cuts, splices and seeds
  tokens into every corpus program with a seeded generator, then reads,
  formats, analyses, compiles and runs each under fuel: nothing may panic,
  and every module written validates. `BS_FUZZ_SEED` and `BS_FUZZ_MUTANTS`
  (12 a program by default) widen a run; `crates/balaur_bs/fuzz/` holds the
  same two checks as `cargo fuzz` targets, `compile` and `run`, for a nightly
  run without end.
- **Random programs** — `tests/suite/random.rs` generates well-typed programs
  and runs each on every engine and on a small interpreter kept for the
  test, which must agree: the engines share one compiler, so only this
  catches it writing the wrong code. 60 a push; `BS_RANDOM_PROGRAMS`. A
  disagreement is shrunk, printed and saved under `tests/programs/random/`,
  so the corpus holds it until it is fixed.
- `scripts/check_scripts.sh` — `balaur check --strict` over the editor, its
  library and templates, and every example: CI runs it on Linux after the
  tests.
- `scripts/e2e_tests.sh` — suites where a full app boots over real sockets
  (`balaur_http`, `balaur_websocket`, `balaur_gamend`, `balaur_platform`), gated
  on `BALAUR_E2E` so a local `cargo test` stays fast. The Gamend ones talk
  to a local Gamend on port 4000 (`GAMEND_URL` overrides): CI starts one with
  Gamend's `setup-gamend` action, and locally `gamend daemon` serves one.

What keeps a run short: the editor and the tests build on one feature set,
`cargo editor` and `cargo suite` (`.cargo/config.toml`), so an edit compiles
each crate once for both, where `-p balaur_cli` compiles every engine crate a
second time; a target whose sources hold no unit tests says `test = false`
(a house lint holds it), so no empty test binary is compiled and linked; a
dev build carries no debug info, so no link goes through `dsymutil`
(`--profile debugging` for a debugger); a long test does
its work on every core (the corpus, the export of every target) and nextest
starts it first (`.config/nextest.toml`); and a generic from a dependency
that does heavy work is called through a concrete type the dependency
compiles itself (`image`'s `DynamicImage`), at opt 3, not instantiated in a
crate built at opt 0.

## End to end, over every example

`scripts/e2e.sh` runs each of the sixteen examples these ways. CI does not
run it; `scripts/precommit.sh --e2e` does.

- **check** — every script a scene attaches, compiled, plus the handle calls
  no compiler can see, and each handler, listener and row naming an event
  nothing announces. The cheapest gate, and the only one that names a file
  and a line rather than a symptom. Run `--strict`, so a warning fails.
  Before the examples, the same gate over the editor, the library it copies
  from and each template it starts a project with: each carries a manifest,
  so each is checked from its own root.
- **run** — dev mode from sources.
- **export**, twice — the export packs must be identical.
- **play** — the export pack, no sources, no compiler.
- **edit** — the editor booted headless against every example. The
  per-project self-test states (`EXAMPLE_STATES`) run on `angrynerds`,
  `hello`, `objects` and `rig`, which between them hold what every state
  needs; elsewhere most would skip. The editor's own states run once.
- **render**, twice — the game and the editor surface-less on a real GPU,
  which is where a texture the backend cannot bind or a shader it rejects
  warns. The step reads the `rendering ... offscreen` line back, so a binary
  built with no `window` feature fails rather than repeating the headless
  run.

Two bars: **a clean exit and a clean log.** A logged `ERROR` or `WARN` fails,
bar a warning a self-test names first with `expect_warning`. The editor's
`did not resolve in the mirror` is why warnings count: when it broke, nested
nodes silently had no ref, so no inspector, no gizmo, no transform read, and
nothing else failed.

`BALAUR_E2E_KEEP_GOING=1` turns the run into a survey: a failure is listed
and the run goes on, so one broken example does not hide the ones after it.
A game checked out beside the engine is checked and run too:
`../polyglot-pirates-app`, or the folders `BALAUR_E2E_PROJECTS` names. Every
run keeps its saves, logs and editor preferences under the output folder,
through `BALAUR_DATA_DIR`, so a self-test never rewrites the developer's own.

Three more runs look where a log does not. `scripts/monkey.sh` feeds the
editor seeded random input, a pointer, clicks, drags, scrolls and keys, from
`scripts/plugins/monkey.bs`, and fails on a crash, a hang, an `ERROR` or a
`WARN`. Fed input reaches what a script reads, the viewport and the
shortcuts; a widget hears the window's own events, so no button is pressed
by it. `scripts/soak.sh` runs each example headless for 600 frames and for
12,000, and fails one whose peak memory grew by more than 32 MB between them.
`scripts/pictures.py` runs each example offscreen at a fixed tick for 200
frames and compares the last one with the PNG in `scripts/pictures/`: it
fails when over 0.5% of the pixels moved, and `--update` writes the set. The
pictures are one GPU's, so the set is the release Mac's. A shader's
`time()` reads `engine::time()`, so an animated material draws the same frame
at a fixed tick. None of the three runs on a push.

Headless covers loading, mirroring, node resolution and asset rebinding; the
render steps cover what drawing reports. What a screen looks like is
`scripts/uiaudit.sh`: one PNG per editor screen, offscreen, catalogued in
`docs/EDITOR-SCREENS.md`. Regenerate and diff before reviewing a shell change.

## Documentation cannot drift

- `cargo doc --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings` —
  mostly intra-doc links, where a rename leaves a dead one.
- `scripts/gen_docs.py --check` regenerates `docs/generated/` from cargo
  metadata and a booted engine, and fails on any diff.
- `api_lints.py` requires a doc line on every script module, function,
  component and asset type.
- `api_lints.py` `hook-name`: a hook the engine tells starts with `on_`, and
  one told a flag's new value ends in `_changed` (N22).
- `scripts/prose_lints.py` fails a roadmap row over one sentence or 25 words,
  and every hand-written `.md` on the mechanical half of the `avoid-ai-writing`
  skill. Sentence length, filler and em dashes are reported on
  `docs/ROADMAP.md`, never failed.
- `scripts/third_party_notices.py --check`. Licence checking is off in
  `deny.toml`, so `THIRD-PARTY-NOTICES.md` is the only record of what a built
  binary combines.

The website builds its reference from the same `api.json`, so the published docs
cannot describe an engine that does not exist.

## Performance

Readings live in `crates/balaur_bench/budgets.toml`, one per benchmark, written
by `scripts/bench.py --record` along with a `[machine]` table naming the CPU,
core count, OS, toolchain and commit they came off. A number with no machine
beside it compares to nothing.

Two ways to read them back. `scripts/bench.py --compare` is the one to use on
the machine that recorded them: it prints every benchmark's shift, worst first,
and fails on anything a fifth slower than its reading. `scripts/bench.py
--check` is the loose one, against a ceiling ten times each reading, for a
machine that is not the recorded one. Nothing in CI gates on either: a shared,
throttled runner times a benchmark badly, and a gate that cries wolf gets
ignored.

A count is not a time, and CI gates on counts. `crates/balaur_bs`'s
`tests/suite/budgets.rs` holds each script operation to the fuel wasmi
charges for it and the host calls it makes, the same on every machine
(PLAN-balaur-script §9 step 8).

`scripts/bench_compare.py` writes `docs/BENCHMARKS.md` from a real run, case for
case against Godot with Rapier, Box2D v3 and Jolt.

`scripts/bench_load.py` asks a different question: what one kind of node, or
one shape of script, costs a frame. Each case is N of one thing under
`examples/benchmark`, built once and then measured as it sits, and the table
says where the frame went — the script, the mirror, the UI pass, the
renderer's two halves. It reports the statements a node's script ran
beside the milliseconds, because that number is the same on every machine and the
milliseconds are not. Run on demand, never in CI, and add a case by writing
one entry in `cases_kinds.bs` or `cases_scripts.bs`.

## Supply chain and releases

- `cargo-deny` on every push: advisories, bans and sources, with `all-features`
  so the audit reaches the windowed tree. Yanked crates, unknown registries and
  git sources are denied, with two pinned forks allowlisted by name and reason.
- Releases carry `SHA256SUMS` and build-provenance attestations, verifiable with
  `gh attestation verify`.
- `build.yml` builds the engine and exports a game with the same published
  actions a player's own repository calls, `build-engine` and `export-game`, so
  a broken action fails here rather than on the day someone else runs it.
- `build.yml` also exports and runs a new project on each packaged template
  (`scripts/package.sh`), runs `scripts/signing_check.sh`
  (`BALAUR_SIGNING_CHECK`) and `scripts/export_check.sh` per platform, and
  `scripts/web_smoke.mjs` over every web export pack through `scripts/package_play.sh`
  and over the web export, booted on the game template, which has no compiler.
  `package_play.sh` boots each example on the web runtime its
  `[export] runtime` picked.
- `build.yml` builds a `-2d`, `-3d`, `-bare` and `-server` template of every desktop,
  exports an example that fits only it onto each with that
  run's editor and plays 60 frames (`VARIANT=... scripts/package.sh`); the
  mobile and web variants go through `scripts/export_check.sh`.
- `scripts/check_web_module.sh` holds both web stagings to what `balaur.js`
  names beside it, wasm-bindgen's `snippets/` included: a static import that
  404s stops the module evaluating, and the build that shipped it is green.

## What review is left

Everything above is mechanical, so review spends its attention on what a script
cannot check: whether the change serves the principles, whether the name is the
right name, whether the test asserts on behaviour a caller can see.
