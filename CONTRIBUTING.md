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
scripts/precommit.sh            # everything CI checks: lints, tests, docs
scripts/precommit.sh --lints    # the lint job alone, when that is all that moved
scripts/precommit.sh --e2e      # adds the socket suites and the example pipeline
```

Install the hook once and a push runs the lints on its own:

```bash
git config core.hooksPath .githooks
```

`docs/QUALITY.md` names every check and what enforces it.

## Speed

The first run of a shape compiles it; the next is incremental. Two settings
carry that cost, and both live on the machine rather than in the repo:

- `sccache` as `rustc-wrapper`, sized past its 10 GiB default. The dependency
  tree does not fit in that, so it evicts what the next shape needs.
- Room on disk. `target/` runs to tens of gigabytes per shape.
