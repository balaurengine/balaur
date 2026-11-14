# Contributing

Maintained by Dragos Daian and Sébastien Crozet. How the project is run:
<https://balaurengine.org/docs/principles>.

`AGENTS.md` is the working guide: the loop from `cargo check` to
`scripts/precommit.sh`, and the rules for comments, tests and prose. This file
is what a pull request is held to.

## What a change is held to

- A change is measured against the principles. Iteration speed and determinism
  are never traded for a feature.
- Propose anything large in a discussion first. The architecture is
  `ARCHITECTURE.md` and `docs/PLAN-*.md`.
- AI-assisted contributions are welcome, at the same bar: you stand behind
  every line, with tests and docs.
- One concern per pull request. Code, tests and docs land together, and
  `docs/ROADMAP.md` moves when the change moves something on it.
- Names follow `docs/NAMING.md`, and a Rust item inside `crates/` the rules
  below; comments and prose follow `AGENTS.md`.
- A change that alters a recorded determinism digest says why.
- Contributions are MIT licensed. No CLA.

## Checks

The toolchain and the per-platform packages a build needs are in `README.md`,
and in full at <https://balaurengine.org/docs/building>. `docs/QUALITY.md`
names every check and what enforces it.

Wall clock on an Apple M1, 8 cores, 16 GB, with `sccache` warm. **Warm** is a
second run with nothing changed; **cold** is the first run of that shape, with
its dependency tree still to compile. Every run prints its own per-stream
times.

| Command | What it runs | Cold | Warm |
| --- | --- | ---: | ---: |
| `cargo check -p <crate>` | that crate and what depends on it | n/a | seconds |
| `cargo check --workspace` | the loop while writing; fingerprints 400 crates | a full build | 23 s |
| `scripts/precommit.sh --files` | rustfmt and the six lints that only read files | 7 s | 7 s |
| `scripts/precommit.sh --lints` | the above, every clippy shape (thirteen on macOS, ten elsewhere) and cargo-deny | a full build | 6 s |
| `scripts/precommit.sh` | the above, plus every test and both kinds of docs | a full build | 48 m |
| `scripts/precommit.sh --e2e` | the above, plus the socket suites and every example project | a full build | 48 m plus the pipeline |
| `scripts/e2e.sh target/e2e hello` | one project through check, run, export, play, edit and render | n/a | 8 m |
| `node scripts/web_smoke.mjs dist/play` | every pack of a web bundle in headless Chrome, 8 s each | n/a | 2 m |

`--fix` on any tier runs `cargo fmt --all` first. Each feature shape keeps its
own target directory, so switching shapes is what rebuilds, not running again.
Warm, the tests are 46 of the 48 minutes, and more cores do not shorten them:
each `balaur` suite test boots a whole app, and that boot is serialised.

Install the hook once, and a push runs `--lints` on its own:

```bash
git config core.hooksPath .githooks
```

CI alone runs the pack and trace comparison across platforms, the
reproducible build, line coverage, and `build.yml`'s template, export and
signing check for every platform. `scripts/coverage.sh` gives the coverage
number locally.

## Speed

Four things carry the cost of a cold shape, and all four live on the machine:

- **`sccache` as `rustc-wrapper`** — sized past its 10 GiB default, which the
  dependency tree does not fit in.
- **Room on disk** — `target/` runs to tens of gigabytes per shape.
  `scripts/clean.sh --sizes` reports what each tree costs, and `--prune` drops
  the incremental caches and what `cargo sweep` finds stale.
- **Free memory** — a full run compiles in one stream while testing in
  another. On 16 GB that reaches swap, and a swapping test takes a minute
  where it takes a second.
- **Gatekeeper, on macOS** — every binary the linker writes is assessed.
  Adding your terminal under Privacy and Security, Developer Tools stops it.

While iterating on one crate, `cargo test -p <crate>` is the cheap loop:
libtest runs a crate's tests in one process, where nextest starts one per test.
`scripts/e2e.sh target/e2e <example>` does the same for the example pipeline.

## Names inside `crates/`

`docs/NAMING.md`'s principles hold for Rust items too, read by a Rust
developer: Bevy's word for an ECS concept, the Rust API Guidelines for the
rest. These rules apply them to what only a contributor meets, and
`scripts/house_lints.py` enforces each it can.

| # | Rule | Lint |
| --- | --- | --- |
| D3, N2 | A typemap type's suffix says who owns it and how long it lives, from the table below. A type that is two of them is split, not renamed. A `*Config`'s pending flag is `changed`, and a `*Snapshot` says in its doc comment what it holds headless. `Settings` is kept for user-authored config on disk and never ends a `Config` | report, denylist ERROR |
| N3 | `Det` marks one thing: a collection with a fixed iteration order standing in for a std type the house lint forbids | ERROR |
| N4 | In CamelCase the dimension is a lowercase `2d` or `3d` at the end, on both of a pair: `PhysicsState3d` beside `PhysicsState2d`. SCREAMING_SNAKE is exempt | ERROR |
| N10, N11, N15 | The verb says what it acts on. `install_*` declares script functions, `register_*` components, `build_*` typemap entries and systems; `*_system` is only what `App::add_system` takes; a backend step is `apply_*` (Config in), `publish_*` (Snapshot out), `flush_*` (drain a Buffer), `pump_*` (OS in) or `sync_*` (ECS to the backend). A group's name is true of everything in it | ERROR |
| N12 | `Fn` ends only the alias of a boxed or pointer callable, no `pub` item is `*Inner`, and a const is named for what it holds | ERROR |
| N13 | One local name per concept: `eng` for `&Engine`, `m` for a binding module; `scripts` are script files or compiled bytes, `script_host` the running host, `script_backend` what builds one | ERROR |
| N16 | A component key names what the scene's author changes, and its registration says in a doc comment what state it writes | report |

D3's suffixes:

| Suffix | Who owns it, and for how long |
| --- | --- |
| `*Config` | Written by callers (scripts, editor, CLI); a consumer applies it and does not own the truth |
| `*Snapshot` | A backend writes it once per frame; everyone else reads |
| `*Buffer` | Anyone appends during the frame; its consumer drains it; empty at frame start |
| `*Request` | One caller inserts it; its handler consumes and removes it |
| `*Registry` | Appended during plugin build only; read-only afterwards |
| `*State` | Owned and changed by exactly one subsystem, across frames; every writer goes through that subsystem's API |
| *(none)* | Immutable after insertion: `ProjectRoot`, `ScriptArgs`, `ProjectManifest` |

Names that bend these, and why:

| Name | Why |
| --- | --- |
| `DetHashMap`, `DetHashSet` | The prefix is the whole job: it says which one the house lint wants |
| `SHAPE_KINDS_2D` | SCREAMING_SNAKE has no lowercase to agree with (N4) |
| `Flat`, `Solid` | A pair that already says which dimension it is, in one file |
| `PostPass::Material` beside `Material3d` | A screen-space pass is not a surface material and has no dimension; the 3D asset took the suffix, which is what tells them apart |
