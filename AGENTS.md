# Working in this repository

Rules for anyone editing here, human or agent. `docs/NAMING.md` governs names
and wins where it disagrees with this file.

## Comments

Prefer a name; a comment is a second thing to keep true. Write one only for:

- **why**, where the reason is invisible — a workaround, an order that matters,
  a value chosen for a reason
- **a contract that would surprise the reader** — `camera_pose` reports where a
  backend put the camera, not what `set_camera` asked
- **a warning** — appending to a binary invalidates a macOS signature

One or two lines: state the constraint and stop. A reason needing a paragraph
is architecture; put it in `ARCHITECTURE.md`.

Never write: a restatement of the line below, a divider or banner,
commented-out code, a doc comment on a test whose name already says it, or
three-plus lines defending a decision.

`scripts/comment_lints.py` enforces the mechanical half across every language
in the tree; a plain-comment block over three lines fails CI.

## Rune

`obj.field = a || b` (and `&&`) **overwrites `a`** when `a` is a local — the
short-circuit result lands in the local's slot too. Parentheses do not help.
Index targets (`arr[i] = a || b`) are affected; `let`, call arguments and `if`
conditions are safe. Compute into a local first:

    let live = split || !document;
    S.viewport_live = live;

A `}` followed by `(` or `[` **continues the expression**: the block's value is
called or indexed, and the error names a type from the line above. Bind first:

    let work = case.run;
    if again { prepare(world); }
    work(world);

## Tests

- A test's name is a sentence about behaviour: `freeing_a_node_frees_its_children`,
  not `test_free`.
- Assert on behaviour a caller can see. A test asserting from inside a script
  carries one control proving the script ran, or it holds vacuously.
- Feature tests and performance tests stay apart. Budgets live in
  `crates/balaur_bench/tests/` and assert orders of magnitude, never
  percentages: a shared runner makes a tight gate cry wolf.

## Changelog

`CHANGELOG.md` gets one line per feature under Added, Fixed or Known issues.
Name what changed and stop — no rationale, no commit hashes.

## Before committing

    ./scripts/lint.sh

CI runs the same checks. Green locally and red on push is a bug in the scripts.
