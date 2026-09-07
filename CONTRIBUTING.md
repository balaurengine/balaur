# Contributing

Maintained by Dragos Daian and Sébastien Crozet. How the project is run:
<https://balaurengine.org/docs/principles>.

`AGENTS.md` is the working guide: how to run a change from `cargo check` to
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
- Names follow `docs/NAMING.md`; comments and prose follow `AGENTS.md`.
- A change that alters a recorded determinism digest says why.
- Contributions are MIT licensed. No CLA.

## Before a pull request

```bash
cargo check                     # the loop while writing
scripts/precommit.sh --files    # fmt and the five lints that only read files
scripts/precommit.sh --lints    # the lint job: every clippy shape, cargo-deny
scripts/precommit.sh            # the above, plus the tests and both kinds of docs
scripts/precommit.sh --e2e      # adds the socket suites and the example pipeline
```

Install the hook once and a push runs the lints on its own:

```bash
git config core.hooksPath .githooks
```

`docs/QUALITY.md` names every check and what enforces it.

## What each check costs

Wall clock on an Apple M1, 8 cores, 16 GB, with `sccache` warm. **Warm** is a
second run with nothing changed. **Cold** is the first run of that shape, with
its dependency tree still to compile. Every run prints its own per-stream
times, so read yours rather than this.

| Command | Cold | Warm | Where the time goes |
| --- | ---: | ---: | --- |
| `cargo check -p <crate>` | n/a | seconds | that crate and what depends on it |
| `cargo check --workspace` | a full build | 23 s | fingerprinting 400 crates |
| `scripts/precommit.sh --files` | 7 s | 7 s | five Python passes, then rustfmt |
| `scripts/precommit.sh --lints` | a full build | 6 s | five clippy shapes, a target tree each |
| `scripts/precommit.sh` | a full build | 48 m | 1493 tests, one process each |
| `scripts/precommit.sh --e2e` | a full build | 48 m plus the pipeline | nine example projects, every editor state each |
| `scripts/e2e.sh target/e2e hello` | n/a | 96 s | one project through run, export, play and edit |

Cold is a build, and a build is the dependency tree. The two feature shapes a
full run added took 13m37s the first time and 31 s after, which is the shape of
every tree here. Nothing above compiles twice: each shape keeps its own
directory, so switching features is what rebuilds, not running again.

Warm, the tests are 46 of the 48 minutes, and they do not get faster on more
cores. Twenty-four of the `balaur` suite take 104 s in one process on one
thread and 107 s on eight: each boots a whole app, and that boot is serialised
somewhere below the test. `--files` and `--lints` cost seconds because neither
runs one.

Three checks stay in CI, because one machine cannot run them: the pack and
trace comparison across platforms, the reproducible build, and line coverage.
`scripts/coverage.sh` gives the coverage number locally when you want it.

## Speed

The first run of a shape compiles it; the next is incremental. Four things
carry that cost, and all four live on the machine rather than in the repo:

- **`sccache` as `rustc-wrapper`** — sized past its 10 GiB default. The
  dependency tree does not fit in that, so it evicts what the next shape needs.
- **Room on disk** — `target/` runs to tens of gigabytes per shape.
  `scripts/clean.sh --sizes` reports what each tree costs, and `--prune` drops
  the incremental caches and anything `cargo sweep` finds stale.
- **Free memory** — a full run compiles in one stream while testing in
  another. On 16 GB that reaches swap, and a swapping test takes a minute
  where it takes a second.
- **Gatekeeper, on macOS** — every binary the linker writes is assessed, and
  `syspolicyd` climbs the process list during a build. Adding your terminal
  under Privacy and Security, Developer Tools stops it.

While iterating on one crate, `cargo test -p <crate>` is the cheap loop:
libtest runs a crate's tests in one process, where nextest starts one per test.
`scripts/e2e.sh target/e2e <example>` does the same for the example pipeline.
