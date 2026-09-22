# Working in this repository

Rules for anyone editing here, human or agent. `docs/NAMING.md` governs names
and wins where it disagrees with this file. `CONTRIBUTING.md` covers how the
project is run and what a pull request carries. `CLAUDE.md` imports this file;
edit this one.

## Where things are

| File | What it holds |
| --- | --- |
| [README.md](README.md) | what the engine is, the toolchain, the quickstart |
| [CONTRIBUTING.md](CONTRIBUTING.md) | what a pull request is held to, and what each check costs |
| [ARCHITECTURE.md](ARCHITECTURE.md) | how the engine fits together, and every decision |
| [docs/NAMING.md](docs/NAMING.md) | the naming rules; wins over every other doc |
| [docs/ROADMAP.md](docs/ROADMAP.md) | what each version holds; the website's roadmap page is built from it |
| [docs/PLAN-*.md](docs/) | one plan per subsystem: what is left, in order |
| [docs/QUALITY.md](docs/QUALITY.md) | every check CI runs, and what enforces it |
| [docs/DETERMINISM.md](docs/DETERMINISM.md) | keeping a game reproducible; record, replay, finding a desync |
| [docs/RELEASING.md](docs/RELEASING.md) | how a nightly, a version and a channel are cut |
| [docs/BENCHMARKS.md](docs/BENCHMARKS.md) | physics timings against Godot, written by `scripts/bench_compare.py` |
| [docs/EDITOR-SCREENS.md](docs/EDITOR-SCREENS.md) | every editor surface: mockup, code, screenshot |
| [docs/actions.md](docs/actions.md) | the GitHub Actions a game's repository uses |
| [docs/generated/](docs/generated/README.md) | script API, components, assets, crates, features; `scripts/gen_docs.py` writes it |
| [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) | every bundled package's licence; `scripts/third_party_notices.py` writes it |
| [editors/code/README.md](editors/code/README.md) | the VS Code extension over `balaur lsp` |
| [editor/library/addons/gamend/README.md](editor/library/addons/gamend/README.md) | the generated Gamend SDK addon |
| [balaur-website](https://github.com/balaurengine/balaur-website) | balaurengine.org: manual, devlog, roadmap page; checked out at `../balaur-website`, with its own `AGENTS.md` |

## How to work

A change opens with `cargo check` and closes with `scripts/precommit.sh`.

1. **Read the plan first.** `ARCHITECTURE.md` says how the engine fits
   together; `docs/PLAN-*.md` holds the work in progress. A plan's own steps
   are the order to do them in.
2. **Loop on `cargo check`,** or `cargo check -p <crate>` for one crate. It is
   the fastest signal while the code is still moving.
3. **Write the test with the code,** not after it. A behaviour with no test is
   a behaviour the next change may delete.
4. **Close with `scripts/precommit.sh`.** It runs what CI's lint, docs and test
   jobs run; `--e2e` adds the socket suites and the example pipeline. Exports,
   signing and the cross-platform comparisons stay in CI. Green locally and red
   on push is a bug in the script, worth fixing there.

A logged warning is a failure. `scripts/e2e.sh` fails a run on any `WARN`, and
`scripts/web_smoke.mjs` fails a pack that logs one in a browser. A self-test
that provokes one on purpose names it first with `expect_warning`.

These land in the same commit as the code:

- **`docs/ROADMAP.md`** changes when the change moves something on it. It is
  the only record of what a version holds. A row is one sentence and at most 25
  words, names the crate or protocol, and carries its milestone: `0.1 done`
  once built, the version alone until then.
- **The plan** it came from, so `docs/PLAN-*.md` says what is left.
- **`docs/generated/`** when the script API moved: `python3 scripts/gen_docs.py`.
- **`THIRD-PARTY-NOTICES.md`** when `Cargo.lock` moved:
  `python3 scripts/third_party_notices.py`.
- **A devlog post** in the website repo's `blog/` when a user can see the
  change. One post per feature, with a picture or a clip.

## Vocabulary

A crate's words and keys live in one `src/vocabulary.rs`: `keys` for the
property names a schema and its reader spell, `words` for the closed sets a
`kind` or a mode takes, and the script constants beside them. A call site
names a constant — `prop_f32(params, k::RADIUS)`, never `"radius"` — so a
schema line and the reader behind it cannot drift apart.
`scripts/house_lints.py` enforces it (`vocabulary-literal`) in every crate
that keeps one; `docs/NAMING.md` N17 says which crates do not yet.

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

`scripts/comment_lints.py` enforces the mechanical half across Rust, Rune,
Python, shell, YAML and TOML; a plain-comment block over three lines fails CI.

## Rune

`obj.field = a || b` (and `&&`) **overwrites `a`** when `a` is a local — the
short-circuit result lands in the local's slot too. Parentheses do not help.
Index targets (`arr[i] = a || b`) are affected, and so is a branch of an `if`
expression assigned to a field; `let`, call arguments and `if` conditions are
safe. Compute into a local first:

    let live = split || !document;
    S.viewport_live = live;

`return || f` is **`(return) || f`**: it returns nothing, and `return |x| ..`,
`return match ..`, `return [..]`, `return crate::m::f()` and a template string
after `return` do not compile. Bind the value first.

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
- Feature tests and performance tests stay apart. Benchmarks live in
  `crates/balaur_bench/benches/` and no CI job gates on them: a shared runner
  times them badly. `scripts/bench.py --compare` reports what moved.
- The suites that boot an app over real sockets gate on `BALAUR_E2E`, so a
  plain `cargo test` stays fast. `scripts/e2e_tests.sh` runs them.

## Writing

Prose in `docs/`, and the devlog posts in the website repo's `blog/`.

- A devlog post is one feature, titled plainly ("Save games"), with a picture
  or a clip. One or two sentences open it, then the bullets. A heading names
  its content, never "What landed".
- A bullet is a plain sentence: what it is, the key or flag that turns it on,
  the number. `fonts = "subset"` keeps the code points a project names; one
  face went from 421 KB to 29 KB. No bold lead closed by a period.
- Describe the feature as it is. What it replaced, or how a first attempt went,
  stays out.
- A claim without a measurement is cut, not softened. "421 KB to 29 KB", never
  "much smaller".
- No throat-clearing: "the honest summary is", "it is worth noting",
  "genuinely", "actually", "truly", "worth a look". State the fact.
- Never invent a number, a flag or a file name. It comes from the code or from
  a run, or it does not go in.
- An em dash is typography only after a list item's lead, `- **Term** — text`.
  Never a prose splice.
- Link the clip or the screenshot where one exists.

The limits are numbers. The website's `scripts/lint-prose.mjs` fails a post
over 300 words of prose, 35 words in a sentence, 60 in a paragraph or 4
paragraphs outside bullets, and a manual page over 35 in a sentence or 90 in a
paragraph. Here, `scripts/prose_lints.py` fails a roadmap row over one
sentence or 25 words, and fails every hand-written `.md` on the mechanical half
of the `avoid-ai-writing` skill: the vocabulary a model reaches for, the
transitions it opens with, the closers it lands on, and the markup its chat
interfaces leak. On `docs/ROADMAP.md` it also reports long sentences, filler
and em dashes, without failing. Run the skill itself over anything longer than
a line before committing it, for the half a regex cannot judge.

A roadmap row says what the thing is, at the level somebody using the engine
reads. Never a date, a plan's phase number, a CI job or a defect id.

The row is also the card on the website's roadmap page, which is generated from
this file. The site's generator only warns, so `scripts/prose_lints.py` is what
holds a row to one sentence and 25 words. Everything the sentence cannot hold
goes where a reader can follow it: what is not planned and why into the
`PLAN-*.md` the row links to, and what shipped into the devlog post the site
pairs with a built row.

## Checks

| Command | What it covers |
| --- | --- |
| `cargo check` | the loop while the code is still moving |
| `scripts/precommit.sh --files` | fmt and the five lints that only read files |
| `scripts/precommit.sh --lints` | the above, plus every clippy shape and cargo-deny |
| `scripts/precommit.sh` | the above, plus the tests and both kinds of docs |
| `scripts/precommit.sh --e2e` | the above, plus the socket suites and the example pipeline |
| `--fix` on any of them | runs `cargo fmt --all` first, so the fmt step checks formatted code |

Each runs the checks in parallel streams, and each feature shape keeps its own
target directory: a shape switch is what rebuilds the world, not a second run.
The run prints how long each stream took, and `CONTRIBUTING.md` holds what a
tier costs cold and warm.

Install the hook once, and a push runs the lints on its own:

    git config core.hooksPath .githooks

`docs/QUALITY.md` names every check and what it is for.
