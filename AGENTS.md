# Working in this repository

Rules for anyone editing here, human or agent. `docs/NAMING.md` governs names
and wins where it disagrees with this file. `CONTRIBUTING.md` covers how the
project is run and what a pull request carries.

## How to work

A change opens with `cargo check` and closes with `scripts/precommit.sh`.

1. **Read the plan first.** `ARCHITECTURE.md` says how the engine fits
   together; `docs/PLAN-*.md` holds the work in progress. A plan's own steps
   are the order to do them in.
2. **Loop on `cargo check`,** or `cargo check -p <crate>` for one crate. It is
   the fastest signal while the code is still moving.
3. **Write the test with the code,** not after it. A behaviour with no test is
   a behaviour the next change may delete.
4. **Close with `scripts/precommit.sh`.** It runs what CI runs. Green locally
   and red on push is a bug in the script, worth fixing there.

These land in the same commit as the code:

- **`docs/ROADMAP.md`** changes when the change moves something on it. It is
  the only record of what a version holds. A row says have, planned, fallback
  or not planned, and names the crate or protocol.
- **The plan** it came from, so `docs/PLAN-*.md` says what is left.
- **`docs/generated/`** when the script API moved: `python3 scripts/gen_docs.py`.
- **A devlog post** in the website repo's `blog/` when a user can see the
  change. One post per feature, with a picture or a clip.

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
- The suites that boot an app over real sockets gate on `BALAUR_E2E`, so a
  plain `cargo test` stays fast. `scripts/e2e_tests.sh` runs them.

## Writing

Prose in `docs/`, and the devlog posts in the website repo's `blog/`.

- Bullets, not paragraphs. One per thing that landed: what it is, the key or
  flag that turns it on, the number.
- Lead with the claim, then the mechanism, then the measurement.
  `**Fonts are cut to what the game shows.**`, then `fonts = "subset"`, then
  421 KB to 92 KB.
- A claim without a measurement is cut, not softened. "421 KB to 92 KB", never
  "much smaller".
- No throat-clearing: "the honest summary is", "it is worth noting",
  "genuinely", "actually", "truly", "worth a look". State the fact.
- Never invent a number, a flag or a file name. It comes from the code or from
  a run, or it does not go in.
- Em dashes stay in `- **Term** — text`, where they are typography. Not as a
  prose splice.
- Link the clip or the screenshot where one exists.

The limits are numbers, and the website's CI enforces them on every post and
manual page: 300 words of prose in a post, 35 words in a sentence, 60 in a
paragraph, 4 paragraphs outside bullets. `scripts/prose_lints.py` holds
`docs/ROADMAP.md` to the sentence rule here, and holds every hand-written `.md`
to the mechanical half of the `avoid-ai-writing` skill: the vocabulary a model
reaches for, the transitions it opens with, the closers it lands on, and the
markup its chat interfaces leak. Run the skill itself over anything longer than
a line before committing it, for the half a regex cannot judge.

A roadmap row says what the thing is, at the level somebody using the engine
reads. Never a date, a plan's phase number, a CI job or a defect id.

## Checks

| Command | What it covers |
| --- | --- |
| `cargo check` | the loop while the code is still moving |
| `scripts/precommit.sh --lints` | fmt, every clippy shape, the lints that read files |
| `scripts/precommit.sh` | the above, plus the tests and both kinds of docs |
| `scripts/precommit.sh --e2e` | the above, plus the socket suites and the example pipeline |

Each runs the checks in three streams, and each feature shape keeps its own
target directory: a shape switch is what rebuilds the world, not a second run.

Install the hook once, and a push runs the lints on its own:

    git config core.hooksPath .githooks

`docs/QUALITY.md` names every check and what it is for.
