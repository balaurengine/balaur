> **Status:** phase 0 is done — clippy, rustfmt and the house, comment and
> API lints are green across the workspace, the macOS Swift abort and the
> Linux build-script failure are fixed, `scripts/lint.sh` and CI run the same
> list, and a `pre-push` hook runs it. Phases 1-3 and 5 landed with the work
> the audit asked for; phase 4 and phase 6 are what is left. Written 2026-09-04
> from an audit of every crate,
> the editor's scripts, the CI and the documents, done by reading rather than
> by running: every line below names a file, and the ones marked *verified*
> were re-read a second time before they were written down. Phase 0 comes
> first because nothing else can be checked while it is open: `main` has
> failed CI on every push since 2026-09-03, so a red job says nothing about
> the commit that made it. The rest is ordered by what a determinism claim
> breaks on first, then by what panics, then by what a shipped build gets
> wrong, then by the tests and the documents.

# Plan: hardening — a green main, and the holes the audit found

The engine promises three things the audit tested against: the same inputs
give the same bits everywhere; a bad file is an error and never a crash; and
what the documents say is what the code does. Each phase below is one of
those promises with the places it does not yet hold.

## 0. Where the tree is today

**CI.** The last eight pushes to `main` (`84f2bd6` through `8b44a17`) all
failed, each for a different set of reasons, and the run for `8b44a17` fails
on six independent causes:

| Job | Cause | Fix |
| --- | --- | --- |
| tests · ubuntu, windows; rustdoc; docs current | `crates/balaur_apple/build.rs:25` calls `swift_rs::` unconditionally while the crate lists `swift-rs` only under `[target.'cfg(target_vendor = "apple")'.build-dependencies]`; on a Linux or Windows host the build script does not compile, and `cargo test --workspace`, `cargo doc` and `gen_docs.py` (which runs the CLI with `--features apple`) all build it | gate the call with `#[cfg(target_vendor = "apple")]`; the build script is compiled for the host |
| tests · macos | `balaur_apple --test api` aborts: dyld cannot find `libswift_Concurrency.dylib`. `.cargo/config.toml` adds `-rpath /usr/lib/swift` through `[target.*] rustflags`, and `test.yml` sets `RUSTFLAGS: -D warnings`, which replaces that table wholesale (the file's own comment says so) | move the rpath to `cargo:rustc-link-arg` in `build.rs`, or stop setting `RUSTFLAGS` in workflows and put `-D warnings` in `[lints]` |
| build web | `package_template.sh:143` runs `wasm-opt --enable-bulk-memory --enable-nontrapping-float-to-int` and it fails with `Fatal: error validating input` after the cargo build succeeded: the toolchain emits a wasm feature the flag list does not enable | pass `--all-features` (or the exact features rustc 1.98 emits) and pin the binaryen version |
| clippy · all three | `crates/balaur_webtransport/src/link.rs:38` `needless_pass_by_value` | take `&Sender` |
| rustfmt | `crates/balaur_ui/src/widget_{measure,layer,bindings}.rs`, `crates/balaur_core/tests/faults.rs` | `rustfmt` those files |
| house rules | `comment_lints.py` finds eight `[essay]` blocks: `Cargo.toml:93`, `.cargo/config.toml:1` and `:15`, `.github/workflows/build.yml:64`, `crates/balaur_render/src/lib.rs:222`, `crates/balaur_script_rune/src/pause.rs:59`, `editor/scripts/session.rn:10`, `scripts/package_template.sh:115` | cut each to two lines; the rationale is already in ARCHITECTURE.md |

Three things let that happen. There is no branch protection and no ruleset
on `balaurengine/balaur`, so a red run blocks nothing. There is no local
hook, and `scripts/lint.sh` is not what CI runs: it runs `api_lints.py`
(which CI does not, and which fails today on `strings` being plural and on
nine undocumented `render.*` and `apple.*` functions), while CI runs three
clippy variants `lint.sh` does not. And the working tree is shared by several
sessions at once, so a push carries whatever state the other sessions left,
`--features apple` or a half-moved helper included.

**Verified elsewhere,** and enough on their own to need this plan: a widget
click changes the digest through state no recording carries; the animation
player is in neither the snapshot nor the digest; a freed node leaves its
colliders in the physics world and a stale handle panics; the audio bus
tree never reaches `master`; the `fs` module writes anywhere; the `f64`
switch enables both scalar widths. Each is in its phase below with the line.

## 1. Rules

1. **`main` is green or the push waits.** A commit that turns a job red is
   reverted or fixed within the hour, whoever made it.
2. **What a replay cannot reproduce is not in the digest.** Either the input
   is recorded, or the state is `readonly` and skipped. There is no third
   state.
3. **A plugin that owns simulation state registers a snapshot source and a
   digest source**, keyed by `StableId`, and a test frees a node and restores.
4. **Content is data.** A scene, an asset, a recording, a save, a packet or a
   HID report that is wrong produces an error line naming it, never a panic
   and never an allocation it chose the size of.
5. **`scripts/lint.sh` is CI**, byte for byte the same steps, or AGENTS.md's
   "the two have drifted, which is a bug in the scripts" applies.

## 2. Phases

### Phase 0 — green, and kept green

1. The six fixes in the table above, one commit each, verified by the run.
2. **Not planned:** branch protection on `main`. Asked for and declined on
   2026-09-04; a red push stays allowed. `draft-release` still runs only on a
   push, so a `workflow_dispatch` from a side branch cannot move the rolling
   `nightly` tag (`draft_release.sh` deletes and recreates it).
3. `scripts/lint.sh` and `lint.yml` become one list: `api_lints.py` and
   `third_party_notices.py --check` join CI; the `window`, `extensions` and
   out-of-tree clippy passes join the script. A `pre-push` hook under
   `.githooks/` runs the script; `git config core.hooksPath .githooks` is
   the one-line install in CONTRIBUTING.md.
4. `house_lints.py:266` hard-codes `&mut App` for `register_*`; the in-flight
   move to `&mut Registry` makes it report 28 errors. The rule learns the
   registry, and `NAMING.md`'s status header stops saying `api_lints.py`
   "does not exist" (it does, since 0.1.0).
5. `scripts/e2e_tests.sh` names `balaur --test mixed`, a Luau-era test that
   is gone; fix or delete the script.

### Phase 1 — what the digest and the snapshot miss

| Where | What | First step |
| --- | --- | --- |
| `crates/balaur_ui/src/widget_layer.rs:881-901`, `crates/balaur_core/src/digest.rs:219-233` — *verified* | `settle_clicks` and `settle_edits` write `clicked`, `active`, `width` and `height` onto the `Widget` component from egui's own event pump, after the tick; `push_components` hashes every property a component's `get` reports, `readonly` included. A click, a seam drag or a tab change moves the digest of a tick no recording can reproduce, and `on_click` handlers never run headless | skip `readonly` properties in `push_components`; then route clicks, focus and seams through `InputSnapshot` so the handlers run on replay; a `replay --verify` test with one clicked button |
| `crates/balaur_anim` — *verified*, no `add_snapshot_source` or `add_digest_source` in the crate | `Playback.time`, `playing`, the queue, every tween and `AnimationState.accumulator` survive a rollback untouched, so a re-simulated kinematic platform pushes bodies from the wrong pose; `App::advance` zeroes only its own accumulator on a record restart (`app.rs:604-607`) | a `snapshot` and a `digest` source keyed by `StableId`; zero the accumulator through a replay-setup hook; a test rolls back a clip mid-play |
| `crates/balaur_core/src/app.rs:258-266`, `scene.rs:250` — *verified* | `Command::Free` despawns without running any component's `remove` hook. Physics prunes `bodies` only (`balaur_physics/src/lib.rs:394`), never colliders, joints, params or wheel inputs, and not at all while paused (`:388`), which is the editor's state; a standalone collider on a freed node answers raycasts forever, and a script that kept `other` from `on_collision_start` panics in rapier's arena one step later | walk the subtree through `ComponentRegistry` `remove` before `free_subtree`; retain every physics map; a test frees a node holding a collider and a joint and queries it |
| `crates/balaur_core/src/snapshot.rs:316-454` | `nodes` restore respawns in `collect_subtree` order, which visits the last child first, so a freed node with later siblings comes back last and `fold` reports "trees differ in shape"; `tests/snapshot.rs:139` frees the last sibling only. A respawned script is re-attached without its `props`, so `init` re-runs with default exports | record the sibling index and the props in `NodeFrame`; a test frees the first of three |
| `crates/balaur_physics/src/lib.rs:288-345` | Bodies, colliders and joints are keyed by entity bits; the node the `nodes` source respawns is a new entity, so `state.bodies = frame.bodies` discards its body and the restored world points at a dead one. `wheel_inputs`, `collider_params` and `joint_params` are in no frame at all | key the frame by `StableId` and re-resolve after `nodes`; add the three maps; a vehicle row in `joints_and_shape_edits_survive_a_snapshot` |
| `crates/balaur_core/src/app.rs:620-623` — *verified* | `Step::Hold` still calls `tick()`, and `advance_time` bumps `tick` and `time`; nothing sets the clock from `Frame.tick`, so after N held frames every replayed tick runs at `tick + N` and a script that branches on `engine::tick()`, the integer DETERMINISM.md tells it to use, diverges | set the clock from the frame, or skip `advance_time` on a hold; a test holds five frames between `begin` and `play` |
| `crates/balaur_core/src/replay.rs:614-650`, `app.rs:684-690` | A frame recorded while the debugger held the root carries the measured `dt`, and nothing re-freezes on replay, so a verified session with one breakpoint hit diverges at the first paused frame | record a frozen frame as `dt = 0`, or a `freeze` source |
| `crates/balaur_core/src/rollback.rs:117-120, 264` — *verified* | `arrived` and `used` are never pruned (only `digests` is), one `Value` per tick per player for the life of a session, and `submit` accepts any tick and any player a peer names | retain both below `ring.earliest()`, refuse unknown players and ticks past `next + INPUT_WINDOW`, bind `player` to the link, `saturating_add` at `netsession.rs:336` |
| `crates/balaur_input/src/lib.rs:271-278` | The gamepad poll skips `is_playing` but not `is_resimulating`, so a rollback re-run reads the hardware | `replay::suppressed` in the poll |
| `crates/balaur_core/src/dap.rs:63` | The debugger opens its own channel, so its commands reach the simulation outside replay and rollback; ARCHITECTURE's item 4 (a lint on `mpsc::channel` outside core) is still the answer | the lint, with `dap.rs` as its one sanctioned site |
| `crates/balaur_script_rune` on the CLI | `balaur run --record` boots with the watcher on and the host never tells `replay` about a reload, so a recording silently spans a code change; only the editor ends the session (`shell.rn:279`) | `pump_reloads` ends an open recording with reason `reload` |

Two of the same kind in physics, smaller: `is_grounded` is a move
(`character.rs:261-267`, `dim2/character.rs:222-228`: a zero-translation
`move_shape` that snaps, writes the transform and pushes bodies), and
nothing says which stage `move_character` may run in. Cache the last move's
`grounded` and give `Engine` a `current_stage()` so both can refuse `update`.

### Phase 2 — panics, limits and confinement

| Where | What | First step |
| --- | --- | --- |
| `crates/balaur_core/src/file_api.rs:15-27` — *verified* | `resolve` returns an absolute path as-is and joins `..` unchecked; `fs.write` creates the parent. A pack's bytecode, or a third-party project opened in the editor, reads and writes anywhere the user can. `save` already refuses a slot called `../../id_rsa`; `fs` is its unguarded twin | canonicalise and `strip_prefix(ProjectRoot)`; a test that `fs.write("../x")` and an absolute path are refused; the editor, which needs the game's root, names it explicitly |
| `crates/balaur_render/src/kiss3d_backend.rs:746, 881` → kiss3d `texture_manager.rs:438` — *verified* | `add_image_from_memory` is `image::load_from_memory(..).expect("Invalid data")`: an undecodable PNG named by a `sprite` with explicit `half_extents`, a `polygon` with authored `uvs` or a `mesh.texture` panics the windowed run, because only the headless size path decodes first | decode in the backend through a fallible helper that logs and falls back to the default texture; name the upload by asset generation so an edited PNG re-uploads (today it never does) |
| `crates/balaur_anim/src/modifier.rs:198-202` | `two_bone_ik` clamps `d` to `(|l1-l2| + 1e-5, l1 + l2 - 1e-5)`, which is `min > max` for a bone under `1e-5` and passes the `1e-6` guard; `f32::clamp` panics | reject `min(l1, l2) <= 1e-5` |
| `crates/balaur_audio/src/bus.rs:69-81`, `lib.rs:437-451, 471-479` — *verified* | `gain` breaks on an empty parent before it reaches `master`, so `set_bus_volume("master", 0)` silences nothing on a declared bus; `reroute` re-applies a slider only to sounds directly on that bus, not on its children; `set_volume` writes the raw volume to the sink and skips the chain | a test with `master = 0.5` and a sound on `ui` under `sfx`; then the three fixes it fails on |
| `crates/balaur_websocket/src/listener.rs:56-71, 137-153`, `frames.rs:93-102` | The listener upgrades synchronously with no read timeout, so one peer that never finishes its handshake blocks every later accept; the client's connect and `read_head` have none either, so a silent server leaves a link `Connecting` forever with no event | `set_read_timeout` before the handshake, `connect_timeout`, one thread per accept |
| `crates/balaur_webtransport/src/lib.rs:95-102, 134, 144, 154` | `max_datagram` is documented as "raised once the session is up" and never is; quinn's `max_datagram_size()` is never read, so a 1199-byte datagram can still be dropped with a warning | `LinkEvent::Open { max_datagram }` |
| `crates/balaur_http/src/request.rs:34`, `lib.rs:165` | `Duration::from_secs_f64` on a per-call `timeout` a script wrote as `inf` panics the worker, whose dropped `Sender` leaves the handler registered and an awaiting task hung; no cap on in-flight requests, one thread each | range-check the call's timeout like the project default; a bounded worker pool |
| `crates/balaur_script_rune/src/packed.rs:59`, `core/heightfield.rs:94`, `glb.rs:122`, `dap.rs:183` | Sizes read off a pack, an asset or a DAP client are allocated before they are checked | check against the source's length first |
| `crates/balaur_export/src/bundle.rs:64, 155` — *verified* | `export_bundle` and `export_macos_app` `remove_dir_all` the output path when it exists, so `-o .` deletes the working directory before writing | remove only a directory that already holds a Balaur bundle |
| `crates/balaur_physics/src/query.rs:173`, `dim2/query.rs:140` | `sort_by(partial_cmp().unwrap_or(Equal))` is not a total order on NaN | `total_cmp`, as `clip.rs:313` does |
| `crates/balaur_physics/src/geometry.rs:139` | `voxelize` takes any resolution and walks `rx·ry·rz` | a ceiling, and an error above it |

### Phase 3 — builds, features and metadata

| Where | What | First step |
| --- | --- | --- |
| `Cargo.toml:43`, `crates/balaur/Cargo.toml:44` — *verified* | The facade sets `default-features = false` on `balaur_physics`, the workspace entry does not, and cargo ignores the first ("could become a hard error"): `cargo tree -p balaur --no-default-features --features f64` shows `default`, `f32` and `f64` all on. The documented f64 recipe compiles both rapier stacks; `balaur_cli` has no `f64` or `parallel` feature; CI builds neither | `default-features = false` at the workspace, `f32` where a build needs it, the two features on the CLI, one CI job per width |
| `crates/balaur_physics/Cargo.toml:37-39` | rapier threads its pipeline only under `all(parallel, not(unsync-callbacks))`, and this crate always enables `unsync-callbacks`; `set_threads` may thread nothing | measure; then either drop the feature or gate the callbacks |
| `Cargo.toml:11` — *verified* | `[workspace.package] authors` names only Sébastien Crozet; the log is 141 commits by Dragos Daian to 13. No `repository`, `homepage`, `readme`, `keywords` or `rust-version`; no `description` on `balaur_cli`, `balaur_export`, `balaur_script_rune`; `balaur_android` is publishable | one metadata pass before anything is published |
| `crates/balaur_input/Cargo.toml:3`, `balaur_http/src/lib.rs:12`, `balaur_gamend/src/lib.rs:12`, `balaur_ui/src/widgets.rs:960`, `docs/NAMING.md:312, 408`, `docs/generated/behaviour.md:665` | Luau left on 2026-09-02; these still say Lua (the input crate's description reaches the website's Crates page) | delete |
| `crates/balaur_script_rune/src/task.rs:138-141`, `pack.rs:78-82` | Saving a file pulled in by `mod name;` does not hot reload — only keys in `state.scripts` are queued and a submodule is folded into its root's unit; `balaur export` compiles every `.rn` as a root, so a submodule naming `super::` fails the export and one that compiles ships twice. No test or example uses `mod` | record each unit's source set; map a change to its roots; export only roots; an example with one `mod` |
| `crates/balaur_cli/src/main.rs:79`, `crates/balaur_export/src/lib.rs:32`, `scripts/package.sh:9` — *verified* | `macos-arm64` is documented as a target; CI builds and `update.rs` expects `macos-universal`, so the flag always ends in "no runtime template" | one name |
| `scripts/package.sh:37, 44` | The shipped editor is built without `extensions`, while `draft_release.sh` and `include/balaur_extension.h:8-10` invite you to drop a C extension into `extensions/` | build with it, or drop the promise |
| `README.md:121` | The quickstart's export line cannot succeed from a source checkout: `templates.rs:278-287` refuses to download for a source build | `--template target/release/balaur` in the line |
| `THIRD-PARTY-NOTICES.md` — *verified stale* | `third_party_notices.py --check` fails, no workflow runs it, and licence checking is off in `deny.toml`, so it is the only licence artefact | phase 0 step 3 |
| `Cargo.lock` | 739 packages, 57 names at two or more versions: `windows-sys` ×5, `objc2` and its whole family ×2, `thiserror` ×2, `base64` ×4, `rand` 0.9 (this tree's own `balaur_websocket`) beside 0.10 | bump the ones this tree controls; `multiple-versions = "warn"` stays |

### Phase 4 — tests and CI coverage

What CI never runs: `f64`, `parallel` (so `scalar_and_threads.rs` never
asserts its thread count), tests under `--no-default-features` (build only),
any `window`-feature test, `apple` under clippy or tests, a single GPU frame
(`--offscreen` appears only in `uiaudit.sh` and `showcase.sh`; the package
smoke ticks headless), `cargo deny check licenses`, MSRV, `bench.py --check`
(4 of 26 budgets gate anything), and the `determinism_trace.sh` matrix covers
`hello` and `angrynerds` only while DETERMINISM.md says "every example" —
`rig`, `rig3d` and `shaders` are the transcendental-heavy ones and are never
traced. `macos-latest` has been arm64 since macOS 14, so the "aarch64 is not
in that matrix" caveat is already false and should become a pinned runner
and a sentence.

1. A GPU job on a software adapter (lavapipe or SwiftShader): `balaur run
   --offscreen --frames 60` on one example, and `render.screenshot` compared
   against a golden. **Still open** — deliberately not added blind, because a
   job that cannot get an adapter turns `main` red for a reason that is not
   the commit's.
2. *Done:* every example is traced in `determinism_trace.sh`, and `test.yml`
   gained a job that runs the physics tests at `f64` and under `parallel`
   (which is the only thing that runs `scalar_and_threads.rs` at all), plus
   a clippy pass over the `apple` feature on the macOS runner.
3. Script-level tests for the modules that have none: `audio.*` and `apple.*`
   have no test that spells a script call; `input.*` has none for actions,
   pads, `feed_*`, touches, `typed` or `dropped_files`; `physics2d` has 49
   of 62 functions uncalled and `physics3d` 63 of 88; `render.*` and `ui.*`
   have the lists in the audit (screenshots, picking, sprites, focus verbs,
   surfaces). The script-driven physics and render suites assert only that
   no error was logged — the AGENTS.md control assertion is missing from all
   of them.
4. Behaviours with no test anywhere: a desync actually detected (`faults.rs`
   asserts `desync() == None`), `presets.toml` in a packed run, a second
   `scene.instantiate` of one file, `settings::faults` with integer values,
   a freed body restored, the CLI recorder across a reload.
5. The Rune lint ARCHITECTURE promises ("the editor can lint for this"):
   `for … in <object>` with no `sort()` before it, and `engine::time()`
   inside `fixed_update`, in `balaur check --strict`. Today `check` forwards
   Rune's own diagnostics and nothing else, and `house_lints.py` walks `.rs`
   only, so the editor's 12,700 lines of Rune are under no length rule.
6. The C ABI parity test asserts six of the header's nine layouts and neither
   side asserts `sizeof(BalaurApi)`.

### Phase 5 — the editor

The in-flight shell migration lands first (its owner's session), and these
ride on it; the first four are defects in the working tree today.

| Where | What | First step |
| --- | --- | --- |
| `editor/scripts/search.rn` | Untracked, while `editor.rn:27` declares `mod search;` — a pathspec commit of the migration without it breaks the editor | `git add` it with the rest |
| `editor/scripts/docks.rn:15-24`, `plugins.rn:116`, `editor/plugins/counter.rn:74` — *verified* | Plugin docks are unreachable: the tab lists are fixed, `registry.docks` is consulted for names only, nothing pushes a plugin id, and `counter.rn` writes `S.dock`, a field that is gone; EDITOR-SCREENS §5 promises one tab per plugin | push each registered dock into `S.docks.bottom.tabs`; assert it in `counterdemo` |
| `editor/scripts/settings.rn:31-44`, `editor.rn:38-136` — *verified* | Preferences load only when the Settings window opens: `init` never calls `settings::load(prefs)` or `apply`, so the theme, `ui_scale`, `sessions/keep`, `verify` and the fault settings are defaults until then; `editor/appearance/compact` has no effect because `editor.rn:252` overwrites `S.compact` every frame | load and apply at boot; make width and the setting combine; an e2e state that writes, commits and re-reads |
| `editor/scripts/settings.rn:20-23, 161, 175-183`, `shell.rn:348` | `settings?<query>` sets `S.settings.query` but not the search field's buffer, which the next frame writes back as `""`; a category click while a query is typed has the same shape | write both, clear both |
| `editor/scripts/layout.rn:146-160`, `center.rn:39, 79, 251`, `chrome.rn:41` | The widget layer draws after `draw_ui`, so rects read back are a frame behind (`ui.widget_rect` is in fact two: `roll_measurements` runs at the start of `widget_layer::draw`, `widget_arrange.rs:55-68`); only `docks::draw` and `status_row` skip a zero rect, so the rail, the tabs, the hooks sheet, the persona bar and the viewport chrome draw at `0,0,0,0` on the first frame and after any size change — D18 again, one frame at a time | roll at the end of `draw`; one zero-rect guard in `style::sheet`; `layoutdemo` asserts no published rect is zero |
| `editor/scripts/layout.rn:201-209` | A `RECTS` log at frame 100, left in, fires in every `uiaudit` and `showcase` run | delete |
| `editor/scripts/showcase.rn:14-15, 218-242` | Click spots are measured off the pre-Stage shell (`ROW = 27` against `left.rn`'s `ROW_H = 23`, tabs at `y = 27`), so the drawn cursor lands off-target in every clip | read the spots off `ui.widget_rect` |
| `layout.rn` and `editor/scenes/main.toml` | Two sources for `GUTTER`, `GAP`, `RAIL_W`, 236, 288, 174 and `BAR_H`; `cmd`, `bar`, `rail` and `status` are still computed; `docks.rn` is not reconciled with the tree | the tree owns every size: `handle` on `body` and `centre`, read `tree_w`, `insp_w`, `dock_h` back — PLAN-ui-layout §5.6 for four lines of TOML |
| `style.rn:48, 58, 63` | `section_label`, `list_row` and `empty` have zero callers; the empty state is hand-spelled about twelve times, the hook pill three, `CONTROL_W` eight; D14's five spellings of "has a script" stand | adopt or delete |
| `scripts/e2e.sh` | `lintdemo` is never run, `breakdemo` and `animdemo` assert nothing, `rotdemo` is referenced nowhere; `layoutdemo` never runs at `scale:2.4` | add them |
| per frame | Up to three `fs::exists` for the brand logo (`chrome.rn:16-23`); `docks::panels` rebuilt about eleven times a frame, re-splitting every script's source in the Script persona; `log::recent(500)` lowercased per frame; the palette list and the settings sidebar rebuilt per frame while open | three small caches |

Defects D10, D11 (dragging), D13 (highlight and clip cue), D14 and D15 in
`docs/EDITOR-SCREENS.md` stand; D5, D7 and D17 are fixed and the table now
says so.

### Phase 6 — the record

One commit, after phases 1–3 land, so it describes the tree as it is:

- ARCHITECTURE.md: `NODE_OPS` is 31 operations, not 28; the replay sources
  are `input`, `gamepad`, `http`, `websocket`, `gamend`, `platform`, `apple`
  and `session` plus the `input_bindings` setup, not "four"; 3D skinning is
  on the GPU with the CPU path as the reference; a pack carries binary
  assets; `Value` has `Bytes`; the trailer magic is `BPAKSELF`; the Transport
  section's "Today: HTTP and WebSocket" predates `balaur_webtransport`;
  kiss3d is a git fork, not `~/work/kiss3d`, and the rune patch is permanent;
  `PROPERTY_TYPES` has `flags` and `node`; the "roughly thirty helpers"
  paragraph and the `app()` escape hatch die with the registry migration;
  "five personas, fixed five-region layout" and "one node" predate Stage and
  the shell tree; CI builds `wasm32-unknown-unknown` with `window`, not
  emscripten headless; "shipped in 0.1.0" rows against a changelog that says
  nothing has shipped; `camera.post` and the settings subsystem have no
  section at all.
- DETERMINISM.md: a snapshot does respawn and free nodes; hot reload is
  handled in the editor only; the two examples use `app.` where plugins now
  say `reg.`.
- PLAN-networking.md §0: `browser.rs` exists, wasm-bindgen only, and is gated
  on `target_family = "wasm"`, which the emscripten web target matches but
  cannot run — so it is unreachable, which is what "not built" meant.
- CHANGELOG.md: a line each for path-addressed settings, the search fields
  and the shell tree; `:137` names `shell.toml` and `arrange.rn`.
- `docs/generated`: regenerate — `settings` still lists `editor_toml`,
  `pages`, `project_toml`; `ui.widget_rect` and `set_widget_surface` are
  missing. Then teach `gen_docs.py` the things it does not know: settings
  groups, presets, replay, digest and snapshot sources, plugin manifests,
  the CLI's subcommands and flags, and the editor's `--state` tokens, so the
  counts above cannot drift again.
- README.md: "Safe Rust throughout" against 225 lines of `unsafe` in seven
  crates; `forbid(unsafe_code)` on the thirteen that have none makes the
  honest sentence — "unsafe only at the FFI edges" — enforced.

## 3. What feeds the other plans

- **PLAN-rapier.md** takes the 2D parity list and the unwrapped rapier
  surface (its items 5 and 6): ARCHITECTURE's "function for function" is not
  true while `physics2d` lacks 21 readers `rapier2d` has, `collider2d.one_way`
  is a no-op (`dim2/collider.rs:262-274` never calls `encode_one_way`), the 2D
  `modify_contacts` hook never reaches a script (`dim2/events.rs:160-220`),
  2D `move_character` drops the node's rotation (`dim2/character.rs:130`),
  and 2D colliders do not round-trip through `get` (`dim2/collider.rs:276`).
- **PLAN-animation-and-resources.md** gets the snapshot and digest sources as
  its first item; **PLAN-input.md** the Bluetooth feature report, the twin
  pads and `reset_bindings`; **PLAN-networking.md** the liveness and journal
  bounds; **PLAN-ui-layout.md** and **PLAN-editor-as-scene.md** close when the
  shell tree lands, and phase 5 is what they leave behind.
- The website's roadmap gains the twin of the ARCHITECTURE row this plan
  adds, and its Crates page loses the word Lua with the input crate's
  description.
