# Working in this repository

Rules for anyone editing here, human or agent. `docs/NAMING.md` governs names;
this file governs everything else. Where they disagree, that file wins.

## Comments

Prefer a name. A comment is a second thing to keep true, and it goes stale
without failing anything.

Write a comment only when the code cannot carry the point:

- **why**, where the reason is not visible — a workaround, an order that
  matters, a value chosen for a reason, a bug this shape avoids
- **a contract that would surprise the reader** — `camera_pose` reports where
  a backend put the camera, so it does not echo back what `set_camera` asked
- **a warning** — appending to a binary invalidates a macOS signature

Do not write:

- a restatement of the line below it
- a divider or banner (`// ---- helpers ----`); the structure is the divider
- commented-out code; git remembers it
- a doc comment on a test whose name already says what it checks

The name is the documentation. `a_freed_node_stops_being_valid` needs no
comment. If a comment feels necessary, first try renaming the thing.

`scripts/comment_lints.py` enforces the mechanical half across every language
in the tree, not just Rust.

## Tests

A test's name is a sentence about behaviour, not a label for a function:
`freeing_a_node_frees_its_children`, not `test_free`.

Assert on behaviour a caller can see. When a test asserts from inside a script,
add one control that proves the script ran at all — otherwise the assertions
hold vacuously when the script never executes.

Feature tests and performance tests stay apart. Budgets live in
`crates/balaur_bench/tests/` and assert orders of magnitude, never percentages:
CI runners are shared and a gate that cries wolf gets ignored.

## Before committing

    ./scripts/lint.sh

CI runs the same checks. Green locally and red on push means the two have
drifted, which is a bug in the scripts, not in the commit.
