> **Status:** planned 2026-10-30; where each workstream stands, 2026-10-31:
> the core split and the ECS are built (PLAN-core-split, PLAN-ecs); the
> script tests are built, random programs with `any`, passing and shrinking
> included, but for step 2's last files and forge's build-size page
> (PLAN-script-tests §2, §9); of the editor follow-ups, the derive is finished, the per-kind
> splits are done for primitives, colliders, joints and soft bodies, and
> every physics builder reads typed components, and `widget` is split into
> components beside it, with the stricter language open
> (HANDOVER-editor-followups); the UI gaps are built, text fields, one
> layout owner, directional focus and accessibility, but for the screen
> reader's platform adapter, which waits on a push to the kiss3d fork
> (PLAN-ui-gaps).

# Plan: foundations

Everything left on this branch, in one order: the editor follow-ups the
handover lists, then four workstreams. The core becomes an ECS framework with
what Bevy's `bevy_ecs` and `bevy_app` give, built on hecs. The core splits
into a kernel and the crates around it. The UI closes the gaps found against
Bevy UI. Balaur Script gets the tests a compiler with four engines needs,
and enums with `match`.

This page is the map. Each workstream has its own plan with the steps.

## 0. The branch

Checked 2026-10-30, after `git fetch origin`:

- Every pull request is merged or closed, #1 to #66; none is open.
- `editor-followups` sits on `origin/main` (c4fc6e85, #66), seven commits
  and the handover commit (a49294a3) on top.
- The local branches with commits `main` lacks are squash-merged PR
  histories. `native-script` is #66's history before the squash: of the
  34,111 lines it added, 15 are not in #66, each rewritten there later.
  `pre-rebase-bench` and `pre-rebase-main` are September backups.
- `followups-audio-bus`, `followups-names-1` and `followups-names-2` are
  merged into `editor-followups`.

Nothing was left to merge.

## 1. Decisions

- **hecs stays** (user, 2026-10-30). Bevy's features are built on hecs 0.11,
  not taken from `bevy_ecs`. All of them can be built on hecs without forking
  it (PLAN-ecs §1). The engine stays ours to fix the same day, determinism is
  designed in, and no outside crate breaks the API four times a year.
- **What `bevy_ecs` 0.20.1 measured against hecs 0.11.2**, Apple M1,
  release, fat LTO, median of 31 to 301 runs:

  | | hecs | bevy_ecs |
  | --- | ---: | ---: |
  | Source lines | 9.5k | 120k |
  | Spawn 100k, one at a time | 11.1 ms | 13.7 ms |
  | Iterate 100k, read only | 278 µs | 274 µs |
  | Iterate 100k, write | 363 µs | 466 µs |
  | Get 10k random entities | 551 µs | 296 µs |
  | Despawn 100k | 4.7 ms | 9.6 ms |
  | 50 empty systems, 10k frames | 1.5 ms | 26.7 ms |
  | Smallest program, wasm, brotli | 25 KB | 103 KB |

  The ECS is not where a frame goes. A script's `node.transform.position`
  write measured 926 ns on 2026-09-21 (PLAN-editor-performance §6f), and a
  hecs lookup is about 55 ns of it.
- **One thread stays.** `Engine` is `Rc` and `RefCell`, and so is the script
  host. Parallel work goes inside a Rust system, native only.
- **One implementation per idea.** Observers are `events::announce`
  listeners. Change ticks feed the change log the redesign built
  (`changes.rs`, `scene.keep_changes`). The derive is how a component is
  declared, so every new component attribute is a derive attribute.
- **Names come from `docs/NAMING.md`.** A hook is the engine calling a
  script (`hooks.rs`) and a state is a look (`states.rs`, `state_machine`),
  so Bevy's "component hooks" and "states" need other words. Each is settled
  in NAMING.md before its code.
- **Rendering is out of scope.**

## 2. The workstreams

| Workstream | What | Plan |
| --- | --- | --- |
| Editor follow-ups | finish the derive, property by index, per-kind splits, a stricter language, the editor native in CI, budgets, the PR gates | [HANDOVER-editor-followups.md](HANDOVER-editor-followups.md), PLAN-property-types §5, PLAN-component-order §4, PLAN-balaur-script §9 |
| Script tests | a corpus of programs on every engine, errors as written, fuzzing, random programs against a reference, CI over the big corpus, a language reference, enums and `match` | [PLAN-script-tests.md](PLAN-script-tests.md) |
| Core split | a kernel and four crates around it | [PLAN-core-split.md](PLAN-core-split.md) |
| ECS on hecs | one door for writes, ticks, lifecycle calls, observers, messages, commands, relationships, required components, schedules, app modes, typed systems | [PLAN-ecs.md](PLAN-ecs.md) |
| UI gaps | text fields shaped like labels, one layout owner, accessibility, directional focus, gradients and shadows | [PLAN-ui-gaps.md](PLAN-ui-gaps.md) |

## 3. Where they meet

- **The derive is the ECS's declaration.** Tracking, required components and
  lifecycle calls are attributes on a derived struct, and `after_write` with
  `Written::Added` or `Changed` becomes a lifecycle call. So the ECS work
  starts once the derive is finished and `PropertyReaders`,
  `PropertyWriters` and `Authored` are gone.
- **The write by index is the door's first half.** PLAN-property-types §5
  makes it "the one way a component changes", for the inspector and scripts.
  The ECS extends the same door to Rust writes.
- **Scripts keep addressing a component by its registry index**, which the
  handover's property-by-index step finishes. Typed systems are for Rust.
- **The `widget` split comes before the text fields.** `field`,
  `text_area`, `drag_value` and `code` become components of their own, and
  the text-field rebuild works on those.
- **The corpus comes before the language changes.** The stricter language and
  enums with `match` both change what compiles, and the corpus is what says
  nothing else moved.
- **One CI job for scripts.** The editor built native (§9 step 7), the budget
  gate (§9 step 8) and the corpus on every engine share a job.
- **The split comes after the per-kind splits and before the ECS.** The
  per-kind splits rewrite components in every crate; the ECS features land in
  the kernel the split leaves.

## 4. Order

Each step lands green: `scripts/precommit.sh --e2e`, the determinism traces,
and the benchmarks compared with the step before. A PR boundary is a place to
stop, open a pull request and merge before going on.

| Step | What | From |
| --- | --- | --- |
| 1 | Review, test and commit the three agents' derive conversions, a crate a commit | handover, in flight |
| 2 | Finish the derive: `bone2d`, `bone3d`, `bindings`; `meta` and `states` stay hand-written | handover 1 |
| 3 | Property by index end to end; a missing component or property does not compile | handover 2 |
| 4 | Per-kind splits: `widget`, `shape2d` to `line2d`, the collider groups, softbody and joint rows, tilemap grids; then delete the old readers, writers and `Authored` | handover 3, 1 |
| 5 | The PR gates, and the `editor-followups` pull request | handover 8; **PR boundary** |
| 6 | Script corpus, every engine, errors as written, fuzzing; CI over the big corpus, the editor native, the budget gate | PLAN-script-tests 1 to 7, handover 5 and 6 |
| 7 | A stricter language: engine records, `map()` and `with` out, typed calls once whole-file builds link units; then enums and `match` | handover 4, PLAN-script-tests 10; **PR boundary** |
| 8 | The core split | PLAN-core-split; **PR boundary** |
| 9 | One door, ticks, change detection, lifecycle calls | PLAN-ecs 1 to 4 |
| 10 | Observers, messages, commands, relationships, required components; the scene tree on relationships | PLAN-ecs 5 to 9 |
| 11 | Schedules, app modes, typed systems, parallel work inside a system | PLAN-ecs 10 to 13; **PR boundary** |
| 12 | Text fields on `balaur_text`, one layout owner, accessibility, directional focus, gradients and shadows | PLAN-ui-gaps; **PR boundary** |
| 13 | Random programs and the reference interpreter; the language reference; Rune leftovers | PLAN-script-tests 8 and 9 |
| 14 | ARCHITECTURE, ROADMAP rows, NAMING rows, generated docs, a devlog post a feature | each step writes its own; this one checks nothing was left |
| any | Reuse Metal buffers in the kiss3d fork | handover 7; needs the user to push `Ughuuu/kiss3d` |

Random programs wait until step 13 so the reference interpreter is written
once, against the language after step 7's changes, not before them.

## 5. Risks

- **Two owners in one worktree.** `../balaur-native` holds uncommitted
  edits from the agents converting crates to the derive. One session owns it
  at a time; the next one starts after step 1 is committed.
- **The split conflicts with any branch cut before it.** No branch is open
  today; one cut before step 8 merges first.
- **A write that misses the door** leaves ticks and lifecycle calls silently
  wrong. The house lint that guards it is written before the migration.
- **Re-entry.** A lifecycle call or an observer that runs a script inside a
  world borrow panics. They queue; PLAN-ecs says where the queue drains.
- **Recorded sessions.** A change in what the digest hashes makes older
  recordings disagree. Each step says whether it moves the digest.
- **Size.** This is the largest change the engine has had. The PR
  boundaries keep each review to one idea.

## 6. Not in this plan

- Rendering: kiss3d, the mirror, GPU work.
- `bevy_ecs`, `bevy_app`, `bevy_reflect` or any Bevy crate.
- Systems on several threads at once, and scripts off the main thread.
- A second scripting language.
