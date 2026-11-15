# Working in this repository

Rules for anyone editing here, human or agent. `docs/NAMING.md` governs names
and wins where it disagrees with this file. `CONTRIBUTING.md` holds what a pull
request is held to, and the checks with what each costs. `CLAUDE.md` imports
this file; edit this one.

## Where things are

| File | What it holds |
| --- | --- |
| [README.md](README.md) | what the engine is, the toolchain, the quickstart |
| [CONTRIBUTING.md](CONTRIBUTING.md) | what a pull request is held to, the checks and their cost, names inside `crates/` |
| [ARCHITECTURE.md](ARCHITECTURE.md) | how the engine fits together, and every decision |
| [docs/NAMING.md](docs/NAMING.md) | four naming principles and what follows from each; wins over every other doc |
| [docs/ROADMAP.md](docs/ROADMAP.md) | what each version holds; the site's roadmap page is built from it |
| [docs/PLAN-*.md](docs/) | one plan per subsystem: what is left, in order |
| [docs/QUALITY.md](docs/QUALITY.md) | every check CI runs, and what enforces it |
| [docs/DETERMINISM.md](docs/DETERMINISM.md) | keeping a game reproducible; record, replay, finding a desync |
| [docs/LIFECYCLE.md](docs/LIFECYCLE.md) | how the app, nodes and scripts live and end, with every state defined; `LIFECYCLE-*.md` for the subsystems |
| [docs/RELEASING.md](docs/RELEASING.md) | how a nightly, a version and a channel are cut |
| [docs/BENCHMARKS.md](docs/BENCHMARKS.md) | physics timings against Godot, written by `scripts/bench_compare.py` |
| [docs/EDITOR-SCREENS.md](docs/EDITOR-SCREENS.md) | every editor surface: mockup, code, screenshot |
| [docs/actions.md](docs/actions.md) | the GitHub Actions a game's repository uses |
| [docs/language/](docs/language/index.md) | the Balaur Script chapter of the site's manual, every example run by the tests; forge copies it |
| [docs/generated/](docs/generated/README.md) | script API, components, assets, settings, crates, features; `scripts/gen_docs.py` writes it. Check `settings.md` before declaring a setting |
| [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) | every bundled package's licence; `scripts/third_party_notices.py` writes it |
| [editors/code/README.md](editors/code/README.md) | the VS Code extension over `balaur lsp` |
| [editor/library/addons/gamend/README.md](editor/library/addons/gamend/README.md) | the Gamend client, a Balaur Script addon generated in `../gamend` |
| [forge](https://github.com/balaurengine/forge) | serves balaurengine.org: the manual and devlog under `content/`, pictures and clips under `priv/static/`; checked out at `../forge`, with its own `AGENTS.md` and `docs/site.md` |

## How to work

1. **Read the plan first.** `ARCHITECTURE.md` says how the engine fits
   together; a `docs/PLAN-*.md` holds the work left, in the order to do it.
2. **Loop on `cargo check`,** or `cargo check -p <crate>` for one crate.
3. **Write the test with the code.** A behaviour with no test is a behaviour
   the next change may delete.
4. **Close with `scripts/precommit.sh`.** `CONTRIBUTING.md` lists its tiers.
   Green locally and red on push is a bug in the script, worth fixing there.

A logged warning is a failure. A self-test that provokes one on purpose names
it first with `expect_warning`; `docs/QUALITY.md` says which runs fail on one.

These land in the same commit as the code:

- **`docs/ROADMAP.md`**, when the change moves something on it. It is the only
  record of what a version holds; a row's rules are under Writing.
- **The plan** it came from, so `docs/PLAN-*.md` says what is left.
- **`docs/LIFECYCLE*.md`**, when a state, or a way in or out of one, is added
  or removed.
- **`docs/generated/`**, when the script API moved: `python3 scripts/gen_docs.py`.
- **`docs/language/`**, when the language moved. Its examples are tested, so a
  page the change no longer matches fails `cargo test -p balaur_bs`.
- **`THIRD-PARTY-NOTICES.md`**, when `Cargo.lock` moved:
  `python3 scripts/third_party_notices.py`.
- **A devlog post** in forge's `content/blog/`, when a user can see the
  change: one post per feature, with a picture or a clip under
  `priv/static/img/blog/`. When `docs/generated/`, `docs/ROADMAP.md` or
  `docs/BENCHMARKS.md` moved, `mix forge.site.sync` in `../forge` copies them
  over and rebuilds the reference and the roadmap page.

## What goes in the engine

Core features only. A behaviour the editor's or a game's scripts can build
from what the engine exposes goes in those scripts: remembering a window's
size, an inspector, a list of recent projects. The engine adds a primitive
where a script cannot reach, and keeps it the smallest general piece.
`ARCHITECTURE.md` "What stays native" says what stays in Rust and why.

## Vocabulary

A crate's words and keys live in one `src/vocabulary.rs`. `keys` holds the
property names a schema and its reader spell, and `words` the closed sets a
`kind` or a mode takes, beside the script constants. A call site
names a constant, `prop_f32(params, k::RADIUS)`, never `"radius"`, so a schema
line and its reader cannot drift apart. The `vocabulary-literal` house lint
holds every crate that keeps one; `balaur`, `balaur_android`, `balaur_bench`,
`balaur_browser`, `balaur_http`, `balaur_net`, `balaur_plugin`,
`balaur_testkit` and `balaur_webtransport` do not yet.

A script does the same. A word it compares or looks up is a `const` at the top
of the file, or in one file of the package when several share it. Where the
engine has the constant, it is that one: `ui::WIDGET_BUTTON`, never
`"widget_button"`.

Words are grouped by what they are, never kept in one long list. In Rust that
is a topic module inside `words`, `words::datatypes::FLOAT`; a script has one
file per topic under `scripts/words/`, `datatypes::FLOAT`. A list short enough
to read at a glance stays flat, and a word only one file spells is a `const` at
that file's top. `scripts/string_lints.py` holds both languages to this;
`docs/QUALITY.md` has its rules.

## Comments

Prefer a name; a comment is a second thing to keep true. Write one only for:

- **why**, where the reason is invisible: a workaround, an order that matters,
  a value chosen for a reason
- **a contract that would surprise the reader**: `camera_pose` reports where a
  backend put the camera, not what `set_camera` asked
- **a warning**: appending to a binary invalidates a macOS signature

One or two lines: state the constraint and stop. A reason needing a paragraph
is architecture; put it in `ARCHITECTURE.md`.

Never write a restatement of the line below, a divider or banner,
commented-out code, a doc comment on a test whose name already says it, or
three-plus lines defending a decision. `scripts/comment_lints.py` enforces the
mechanical half; `docs/QUALITY.md` has its rules.

## Balaur Script

The traps a newcomer meets first. `docs/language/` is the whole language.

- A block followed by a line opening with `[` or `-` is read as one
  expression: a list closing a function after an `if` is the `if` indexed.
  Write `return [..];`.
- A `{` in a string starts an interpolation; `{{` and `}}` write braces.
- A plain method cannot await. To hear a call's reply it starts an async
  method of its own, `self.node.call("await_reply", id)`, which runs once the
  plain method has returned.
- An unmarked item is its file's own, `internal` its package's (the
  `addons/<name>` or `deps/<alias>` it sits in, else the project), `pub`
  everyone's. A `pub` free function is also an entry point for the engine, and
  a `pub` field with a default is what a scene tunes, so a field other files
  only read is `internal`.
- A host function's error **ends the whole script call**, and nothing catches
  it. A typo should: a component or property nothing has, a node that is gone.
  Where a failure is expected the host answers nil or `false` instead
  (`toml::parse`, or `node.patch_component` on a value the component refuses,
  which logs `write refused`), and the caller checks.

## Threads

A native thread sleeps until its work arrives: a socket ready, a command
queued, a deadline due. It never wakes on a timer to look. A socket sits on
`mio` with a `balaur_core::wake::Commands` queue, blocking work goes to a
`balaur_core::task::Pool`, and a deadline asks `balaur_core::wake::at`. The
`timed-wait` house lint fails a sleep or a socket timeout outside tests with
no comment saying why.

## Tests

- A test's name is a sentence about behaviour: `freeing_a_node_frees_its_children`,
  not `test_free`.
- Assert on behaviour a caller can see. A test asserting from inside a script
  carries one control proving the script ran, or it holds vacuously.
- Feature tests and performance tests stay apart. Benchmarks live in
  `crates/balaur_bench/benches/`, and no CI job gates on them.
- The suites that boot an app over real sockets gate on `BALAUR_E2E`, so a
  plain `cargo test` stays fast; `scripts/e2e_tests.sh` runs them.
- The Gamend suites talk to a local Gamend on port 4000, or to `GAMEND_URL`;
  `BALAUR_E2E_GAMEND=0` skips them. Never gamend.org: it is a live site, and
  a ten-second stall there failed four tests on main. `gamend daemon` serves
  one locally.

## Writing

Prose in `docs/`, and the devlog posts in forge's `content/blog/`.

- A devlog post is one feature, titled plainly ("Save games"), with a picture
  or a clip. One or two sentences open it, then the bullets. A heading names
  its content, never "What landed".
- A bullet is a plain sentence: what it is, the key or flag that turns it on,
  the number. `font_recode = "subset"` keeps the code points a project names;
  one face went from 421 KB to 29 KB. No bold lead closed by a period.
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

Forge's `mix forge.site.lint_prose` fails a post over 300 words of prose, 35
words in a sentence, 60 in a paragraph or 4 paragraphs outside bullets. It
fails a manual page over 35 words in a sentence or 90 in a paragraph. Here,
`scripts/prose_lints.py` fails every hand-written `.md` on the mechanical half
of the `avoid-ai-writing` skill. Run the skill itself over anything longer
than a line before committing it, for the half a regex cannot judge.

A roadmap row is one sentence of at most 25 words, which `prose_lints.py`
holds it to. It says what the thing is, at the level somebody using the engine
reads, names the crate or protocol, and carries its milestone: `0.1 done` once
built, the version alone until then. Never a date, a plan's phase number, a CI
job or a defect id. It is a feature big enough for a devlog post of its own.
Smaller work belongs to the row it serves, or to none, and a post with too
little to say is a line in a bigger one. The row is also the card on the
site's roadmap page. What the sentence cannot hold goes where a reader can
follow it. What is not planned, and why, goes into the `PLAN-*.md` the row
links to, and what shipped into the devlog post the site pairs with a built
row.
