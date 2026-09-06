# Contributing

Maintained by Dragos Daian and Sébastien Crozet. How the project is run:
<https://balaurengine.org/docs/principles>.

- A change is measured against the principles. Iteration speed and determinism
  are never traded for a feature.
- Propose anything large in a discussion first. The architecture is
  `ARCHITECTURE.md` and `docs/PLAN-*.md`.
- AI-assisted contributions are welcome, at the same bar: you stand behind
  every line, with tests and docs.
- One concern per pull request. Code, tests, docs and a `CHANGELOG.md` line
  land together.
- Names follow `docs/NAMING.md`; comments follow `AGENTS.md`.
- A change that alters a recorded determinism digest says why.
- Contributions are MIT licensed. No CLA.

## Before a pull request

```bash
scripts/lint.sh                 # what CI runs: fmt, clippy, house lints, notices
cargo test --workspace
python3 scripts/gen_docs.py     # regenerate docs/generated; CI fails on drift
```

Run the first on every push: `git config core.hooksPath .githooks`.

`docs/QUALITY.md` lists every check and what it is for.
