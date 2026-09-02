# Contributing

Balaur is built by its maintainers, Dragos Daian and Sébastien Crozet, with
contributions that serve its principles: fast to run, fast to iterate, easy
to use, always deterministic. The full list, and how the project is run, is
at <https://balaurengine.org/docs/principles>. The short version:

- A change is measured against the principles. Iteration speed and
  determinism are never traded for a feature.
- Propose anything large in a discussion before building it. The maintainers
  own the architecture; `ARCHITECTURE.md` and `docs/PLAN-*.md` are where it
  is written down.
- AI-assisted contributions are welcome, at the same bar as any other: you
  understand and stand behind every line, and the change comes with tests
  and docs.
- One concern per pull request. Code, tests, docs and a `CHANGELOG.md` line
  land together.

## Before you open a pull request

```bash
scripts/lint.sh                 # fmt, clippy, house and comment lints — what CI runs
cargo test --workspace
python3 scripts/gen_docs.py     # regenerate docs/generated; CI fails on drift
```

Names follow `docs/NAMING.md`; comments are one or two lines stating a
constraint, with the rationale in `ARCHITECTURE.md`. A change that alters a
recorded determinism digest has to say why.

Contributions are licensed under the project's MIT license; there is no CLA.
