> **Status:** written 2026-11-13 from a review of the script API, an audit of
> what still polls, and a run of every example and editor state at `aea99a80`.
> §3's decisions are the owner's, taken the same day. A line says when its
> item is built; §4 is open from its second item, and §5.2 in part.

# Plan: the alpha

What stands between the tree and an alpha a stranger can use. The features
are in; this plan adds none. It fixes what a run found broken, lands the API
breaks while a rename costs nothing, replaces what still polls, and adds the
tests that would have caught each defect.

## 0. Where the tree is

Measured on a debug build of `aea99a80`, on a Mac.

| Run | Result |
| --- | --- |
| 15 examples, each through check, run, export, play, render and edit | 90 steps, each a clean exit and a clean log |
| 134 editor self-test state runs in five suites | 133 pass; `test:inspector_rows` fails with five suites at once and passes alone |
| 10 editor sessions of 2,500 frames of random input | no crash, no error, no freeze |
| 12,000 headless frames of five examples | four hold near 55 MB; `interface` peaks between 65 and 275 MB |

The script API is 112 components, 51 modules and 566 functions. 143 of the
functions have no script caller and no test.

Measured again on 2026-11-14, a debug build of the tree with §2 and §3 built.

| Run | Result |
| --- | --- |
| `scripts/precommit.sh` | every stream passes |
| `scripts/e2e.sh` over 16 examples, the editor's states and Polyglot Pirates | 234 steps pass; `test:state_machine` on `rig` failed, which was defect 15 |
| `scripts/pictures.py` | 16 of 16 examples match their committed frame |
| `scripts/soak.sh` | every example peaks between 11 and 26 MB, at 600 frames and at 12,000 |
| `scripts/monkey.sh`, two seeds over six examples | 12 sessions, no crash, error or freeze |
| `test:recordings` on `rig` | a replay parted from its recording at a tick that differed per run, which was defect 16; it now passes on every example the states run on |

## 1. Rules

1. No feature lands until §2 to §5 are done.
2. A rename is a clean break: no alias, no migration.
3. A defect is fixed with the test that would have caught it.
4. Polyglot Pirates builds against every engine change (§6).

## 2. Defects

In the order they get fixed.

1. **Polyglot Pirates does not compile.** Built: its cues play through
   `audio::play_file`, its alert is a `panel` with `on_top`, and its Gamend
   and save addons are the library's. `balaur check --strict` is clean.
2. **`examples/text` draws its 3D text wrong.** Built: the glyph atlas
   doubles when it fills, `balaur_text` places a block again when the atlas
   grows under it, and `text3d` rebuilds a block built under a smaller
   atlas. Not checked: a bitmap font's page region, which may hold the same
   fault.
3. **`test:inspector_rows` depends on load.** Built: the Inspector refills
   its whole form 0.3 seconds after a change, and the test waits on that
   deadline rather than on a count of frames, which load stretches past it.
   The deadline itself is §4's first item.
4. **`examples/interface` does not hold its memory.** Built: the Balaur
   Script compiler releases a value it hands to the engine, on a property
   write and on a `str?`, a struct or a class passed to a host call.
   `interface` holds 18 MB from 6,000 frames to 48,000.
5. **A self-test writes the user's own data.** Built: `BALAUR_DATA_DIR`
   names the per-user data folder of a run, and `scripts/e2e.sh`, `monkey.sh`
   and `soak.sh` point it under `target/`, so the states leave
   `balaur-editor/editor.toml` under the real home folder alone.
6. **A scene of widgets opens to an empty viewport.** Built: the Scene
   workspace draws the world and no widget, so a scene holding widgets and
   nothing of the 2D or 3D world opens in the UI workspace. `test:every_node`
   checks which rows make such a scene.
7. **A run's picture is paler than its scene.** Built: the film is linear
   light, and `balaur_render::film` decodes each authored colour where the
   backend hands it to kiss3d, so the resolve encodes each value once and a
   `#3c4753` shape is `(60, 71, 83)` in a shot. The default `ambient_color`
   and the four scenes that write one are the same light, written as a
   colour. Text is decoded the same way. Not done: a shader's `[params]`,
   which are its own numbers; and the fork's resolve, a 2.2 power where a
   sampler decodes by the sRGB curve, so a texel of 60 is drawn 62.
8. **`balaur check` passes a scene it cannot load.** Built: `check` builds
   every scene file with the loader a run uses, its scripts held, and reports
   what the loader refuses as an error on the line that writes it, naming the
   node, the component and the property.
9. **No gate compiles the library's addons.** Built: `check` compiles and
   lints every `.bs` file under `addons/` that no scene reached, an addon's
   `editor/` plugins included, and reports each under its own name.
10. **`examples/benchmark` fails when the editor plays it.** Built: a scene
    the editor plays sits under the editor's own, so its script reaches its
    cameras from its own node rather than by a path from the root.
11. **A game may call what only the editor has.** Built: a module the
    editor's binary declares is `editor_only()`, each function marked where it
    is declared; `dependency` keeps `list` and `of` unmarked. Only the
    editor's files may call one: the project `balaur edit` runs as the editor,
    and anything under a game's or an addon's `editor/` folder. Any other file
    fails in `balaur check`, in Play and the Problems list, and in an export,
    and so does a `use` of an editor file or a scene attaching one. `balaur
    run` says "the engine has no `debugger::scope`", since a runtime declares
    none of the editor's names. An export leaves the editor's plugins out.
    `check` compiles every script `ignore` keeps, attached or not, and the
    save addon's `saved` is heard by the addon's own plugin. The judgment
    costs 0.7 ms of a 300 ms warm boot.
12. **Exporting the editor warns.** Built: the library is a project of its
    own, and a material inside a nested project is left to that project, so
    an export of the editor links none of the library's materials against
    the editor's root and warns of no shader under `editor/shaders/`.
13. **Nine self-test states ran nowhere.** Built: `scripts/e2e.sh` runs
    `test:tools`, `test:lint`, `test:palette`, `test:panels`, `test:imports`,
    `test:deselect`, `test:viewport_clicks`, `test:gestures` and
    `test:state_machine`, and every check in them passes: a completion
    carries a doc line, hover answers on a method's own name, `test:imports`
    runs to its end, `test:gestures` reads the 2D camera in a 2D scene, and
    the dock's "add state machine" writes a property that exists.
    `test:manager` stays out: it passes only when the binary sits in cargo's
    build folder, and e2e runs a copy.
14. **`balaur_cli`'s scene tests failed at random.** Built: they share the
    process's log buffer and hold one lock, so one test's `clear` cannot wipe
    the error another is about to read.
15. **A dock panel opened empty.** Built: each of the fifteen panels that
    fills on show also refills from its own node's `visibility_changed`, so
    a panel that hears `docks_changed` while still hidden fills when it turns
    visible. `test:state_machine` on `rig` checks it.
16. **A recorded session of `rig` did not replay.** Built: the frame a
    recording starts in is not recorded, and a replay holds the frame it
    loads in, so `start_recording` holds the game until the next frame. A
    tool's tweens count apart from the game's, and the header restores the
    game's counter, so the tween `fall.bs` starts on tick 207 takes the same
    handle recorded and replayed. `test:recordings` runs on every example the
    states run on.
17. **`test:timings` compared two frames.** Built: the window's period is
    noted before the frame advances, and `TimingLog` pairs it with the frame
    it measured, so a script reads one frame's stages against that frame's
    period.
18. **A debug build can panic in a window that cannot present.** Built:
    egui checks that an id keeps its layer, so the sensors every node gets
    are keyed by layer, the tooltip id `balaur-tip` and a `menu`'s rows in
    each popup that shows them included. Fields, `code` nodes, seams and
    hints are keyed by layer too, and a field moved to another root keeps its
    focus and caret; a selection, held in memory and no widget, stays the
    node's.
19. **A replay asked the machine for its audio device.** Built: the answers
    of `device_ready` and `devices` are recorded, `init`'s in the header, and
    a replay answers from them, so a script that branches on
    `audio::device_ready` takes the same branch on a machine with other
    speakers or none.
20. **A replay reads `cmd` in a binding as its own machine does.** Built:
    the header's setup carries whether `cmd` is ⌘, amended in the frame a
    recording opens, and a playing replay's bindings read that whatever the
    browser backend reports, so a Mac recording replayed on Linux answers
    `input::action_*` for `cmd+KeyZ` as ⌘.
21. **A replay's `init` read another dark mode.** Built: `balaur run` seeds
    `DeviceFacts::dark_mode` before `init`, and `balaur replay` begins
    playing first, so the device facts are in the header's setup, amended by
    a report in the frame the recording opens in, and a replay restores them
    before `init`.
22. **An `internal async fn` could not be called from another file.**
    Built: a unit gives the units that call it an async function's `$start`
    and `$poll` when it is `pub` or `internal`, an `internal` one's going to
    its package as a plain function's do.
23. **A platform could tick at its own rate.** Built: loading refuses an
    override of a setting whose spec says `overridable = false`, as
    `[time] tick_hz` does, and the rate is read from the base value, so a
    phone steps at the rate its recordings and matches assume.
24. **A game the editor plays read the editor's files.** Built: its scripts
    are keyed from the game's root, and a script keyed inside a root the host
    added reads, writes and lists under that root, or its dependency's
    folder, so `fs::read("scenes/flat.toml")` reads the game's file. The
    editor's own scripts are keyed relative and keep its root.
25. **A replay took the project's tick rate.** Built: a replay keeps the
    header's rate when the project loads after it, and a recording opened
    before the project loads, as `balaur run --record` opens one, takes the
    rate the project sets into its header.
26. **`task::seconds` woke a step late.** Built: a wait is counted in steps,
    `ceil(seconds * tick_hz)` with a product within 1e-6 of a whole step
    taken as that step, so 4/60 takes 4 steps and a pause holds it as it
    holds `task::frames`.
27. **Two file-watcher tests failed in a full suite run.** Built: macOS
    starts about 512 FSEvents streams a process and a new one past them hears
    nothing, so an `App` takes its script host out of the engine as it drops,
    and the host's stream goes with it.
    `script_reload::a_watched_directory_says_what_changed_and_reloads_its_scripts`
    and `ten_thousand_files_changed_at_once_arrive_in_a_handful_of_lists`
    write once and wait for the event itself, capped at a minute of ticks.
28. **A replayed tick lost the snapshot's one-frame fields.** Built: the
    per-tick restore counts the frame as begun and keeps the machine's own
    fields, the command key, pointer emulation and whether a script read a
    focus key, so `just_pressed`, `just_released`, mouse delta, scroll, typed
    text, dropped files and touch starts and ends hold through a replayed
    tick, and `input::key_just_pressed` answers as recorded.
29. **Clicks drawn during a replay landed after it.** Built: a replayed tick
    drops the draw's widget report; a re-simulated one leaves it to the live
    tick after it. Only the subtree `debugger.set_scope` names takes the
    recording's widget input, and every other widget takes the draw's, so the
    Session dock's pause and step work while a session plays, and
    `test:recordings` clicks both buttons. A replay's own pause keeps the
    part of a step the recording carried, so a windowed replay keeps to it on
    the tick after it resumes.
30. **A `Node` field took no path from a scene.** Built: a path in a `Node`
    or `[Node]` field, as `target = "../Enemy"` in `[nodes.script.props]`,
    is read from the scripted node as `node.get_node` reads one when the
    props are applied, and a path naming nothing warns with the field and the
    path. A snapshot restore runs the respawned nodes' scripts once every
    node is back, so a path to a node respawned after its own resolves and
    warns of nothing.
31. **A windowed replay parted from its recording after a resume.** Built:
    the app, the animation player's playhead and the jiggle springs all ask
    `Engine::hold_keeps_remainder`, so each keeps its fixed-step remainder
    through a paused replay, and a clip, as hello's platform is, steps on its
    recording's frame after a resume. A window's frames are rarely whole
    steps and an offscreen run's always are, so only a windowed run shows it.
32. **Four editor states failed only in a window.** Built. A frame whose
    surface hands out no texture, as a sleeping display's does, runs through
    all but the GPU's work and the present, so egui's pass ends and
    `test:viewport_clicks` passes; kiss3d spends the two-second wait for a
    first surface once. `BALAUR_NO_PRESENT=1` runs a window that way. The
    other three states fit any window: `test:layout` takes a compact window's
    20-wide dock fold, `test:gestures` feeds fingers in physical pixels, as
    touches are, and `test:inspector_rows` clicks a row above the fold of a
    short window.
33. **The editor's clock stood still while a recording was loaded.** Built:
    `facts::read_clock_system` stops reading the machine while a replay is
    loaded, so the machine's clock is read every tick into `MachineClock`,
    never recorded, and `engine::unix_time` answers it to a script on a node
    outside `debugger::set_scope`'s subtree, as tweens and rest probes tell a
    tool from its game. The editor's own deadlines, a lint sweep or a
    refill, run on it, and the game reads its recorded time.
34. **A held game froze the editor's own animation.** Built: under a
    debugger stop or a paused replay, `advance_system` holds only the clips,
    tweens and machines inside `frozen_root`. Those outside step on
    `held_accumulator`, so the game's remainder stays as defect 31 keeps it.

## 3. The API

Decided 2026-11-13. Each item breaks scripts, so each lands with the editor,
the library, the examples and Polyglot Pirates changed in the same commit.

1. **One way to hear an event.** A script hears an event in its
   `on_<name>(payload, source)` method: its own node's unasked, another
   node's after `events::listen`. A scene sends one to a script with a
   binding row, a widget's click is `pointer_click`, and the names take
   `docs/NAMING.md`'s forms. A script
   declares the events its node sends, and `balaur check` warns on a handler
   or a listener naming an event nothing announces. `PLAN-events.md` §6 holds
   the design. Built: the handler shape, the `call` row that climbs to
   its method, every scene's handlers as rows,
   `ui.write` keeping a control table's handler names, `pointer_click` for a widget's
   click, the `event` item, the check's warnings, the hooks page's
   listener-only events, and every name in `docs/NAMING.md`'s form.
2. **Editor leftovers leave the kernel.** Built: presets are the editor's
   `presets.toml`, which the picker builds with `set_component`, and a
   game's `editor/` and each addon's add their own. `dependency::install`,
   `add_path`, `make_local` and `move_into` are `balaur_cli`'s, beside
   `project` and `forge`. `balaur::ToolsPlugin` holds `debugger` and the
   `script::` tool verbs, which the editor's binary registers and no game's
   runtime does.
3. **The demo plugin leaves the editor.** Built: the Counter dock's plugin
   is `scripts/plugins/counter.bs`, which `scripts/e2e.sh` copies into the
   project it runs `test:plugin_docks` on.
4. **A handle method does not repeat its component.** Built:
   `softbody2d.area()`, `joint2d.force()`, `vehicle3d.speed()`.
   `balaur_script::handle_method` names every handle's methods, and one that
   would spell a property takes a noun of its own, as
   `collider2d.mass_properties()` does. A soft body sleeps and wakes with the
   body's own `is_sleeping` and `wake_up`.
5. **Vectors in, vectors out.** Built: a function that takes a point, an
   offset, a size, a direction or a velocity takes a `Vec2` or a `Vec3`, and
   one that takes a colour takes a `Color`: `set_linear_velocity(Vec2)`,
   `node.set_tint(Color)`, every `render::draw_*`. An answer of several
   numbers is a record of vectors: a collider's `aabb()` holds `min` and `max`,
   `render::mouse_ray()` an `origin` and a `direction`, `camera_pose()` an
   `eye` and a `target`, `camera_2d()` a `center` and a `zoom`, and a
   character's move the offset it `applied`.
6. **`camera3d` aims by its node's rotation,** as `light3d` does. Built:
   `transform.look_at(point)` turns any node to face a world point. `focus_distance` is how far ahead an
   orthographic frame with no height is sized and a stereo pair converges.
7. **3D immediate shapes say so.** Built: `draw_box_3d`, `draw_sphere_3d`,
   `draw_capsule_3d` and `draw_lines_3d`, beside `draw_rect_2d`.
   `draw_box_3d` takes the box's whole `size`, as `draw_rect_2d` does, and
   `draw_lines_3d` a list of `Line3d` records.
8. **Fed input is a setter beside its reader.** Built:
   `set_mouse_position`, `set_mouse_down`, `set_key_down`,
   `set_scroll_delta`, `set_action_value`, `set_touch` and `set_gamepad`,
   each taking a `Vec2` for a point.
9. **No surface repeats an event.** Built: a script hears the `changed`,
   `files_dropped` and `on_setting_changed` events, and the API holds no
   reader that answers the same, nor a call nothing makes. Kept, each the
   only reach a script has: the `ui::focus_*` verbs and
   `engine::environment`. The focus verbs have no test.
   `node.component.x = v` compiles to the property's own binding, and
   `patch_component` is the one spelling of a partial write.
10. **An example plays sound.** Built: `examples/sound` loops a positional
    hum, rings a bell from a `timer` and counts its `sound_finished`, and
    plays a thud with `audio::play_file` when a ball lands. `scripts/make_sounds.py`
    writes its three WAV files.
11. **`docs/NAMING.md` says why.** Built: four principles open the file,
    and every rule sits under the one it follows from. They are the reader's
    words (Godot, then Unity, then a domain's standard, then plain English),
    one word for one meaning, one shape per kind of thing, and a name that
    says what the call site cannot. An event names what happened, in the past
    tense, and input keeps Unity's pairs. Where a backend has a word of its
    own, the reader's word wins: `point_size`, `stereo_separation`, `tab_size`,
    `specular_shininess`, `normal_scale`, `torn`, and a character's `exclude`.

## 4. What still polls

Every engine thread sleeps on its event. What is left reads a value each
frame to see whether it moved.

1. **A sleeping loop keeps time.** Built: before it sleeps, a
   `low_processor` loop asks `balaur_core::rest`, where each subsystem
   registers one probe. Frames keep coming while a `task` wait or a `timer`
   counts, a clip or a tween plays, a body is awake, particles live or a
   sound is due to end. The editor's autosave, lint sweep, Inspector refill
   and import list each ask `util::wake_at` for one frame at their deadline.
   After one edit an untouched editor draws 7 frames in 6 s, and 2 frames
   in 2 s while an import list comes down. A
   `task::seconds` wait holds the loop awake while it counts, so none uses one.
2. **The editor's file cache.** Built: `model.bs` keeps a read and a listing
   until `files_changed` names the file, and the editor's own writes forget
   theirs, so nothing re-reads on a timer.
3. **The recordings list.** Built: `session.bs` lists the folder again when
   `files_changed` names a file in it, and follows playback through the
   `replay_state_changed` event. A replay paused or run out asks for no
   frame: one loaded and paused draws 19 frames in 12 s, all of them its
   opening.
4. **Transforms and the scene mirror.** Built: `scene::sync_globals` places
   only the subtrees under a stamped `transform`, an attach, a restyle or a
   blend, and the mirror redraws only the renderables those and the change
   ticks name.
5. **Freed nodes.** Built: physics and audio drop a freed node's bodies,
   colliders, joints and voices from the list `scene::on_freed` hands them as
   the free runs.
6. **The device.** Built: dark mode, the safe area, the keyboard and the
   text scale are asked once, then again only after a resize, a focus or
   visibility change, winit's `ThemeChanged` and scale events through
   kiss3d, mundy's subscription on Linux and Windows, or a page's
   `matchMedia`, `resize` and `visualViewport` listeners. Still to go: an
   iPhone's dark mode, safe area and keyboard and Android's safe area and
   keyboard reach no event, and stay asked every frame.
7. **Pads on Windows.** Built: the pads open on the first read, a script's
   `input.gamepad*` or an action bound to a pad, so a game that reads none
   never starts gilrs. Once open, gilrs still reads every pad every 8 ms:
   Windows.Gaming.Input has events only for a pad arriving or leaving.
8. **A browser tab at rest.** Built: a `low_processor` tab awaits a promise
   that a wake, input, the repaint due or a `wake::at` deadline settles, and
   asks for no animation frame while it waits.
9. **The `timed-wait` lint.** Built: it also fails `recv_timeout`,
   `wait_timeout`, `park_timeout` and `tokio::time::interval`.

## 5. Tests

1. **Every push.** `scripts/precommit.sh`, as now.
2. **Every night.** Built: `BALAUR_E2E_KEEP_GOING=1 scripts/e2e.sh` does
   not stop at the first failure, `scripts/monkey.sh` feeds seeded random
   input to the editor, and `scripts/soak.sh` fails an example whose memory
   at 12,000 frames is past its memory at 600 by more than 32 MB. None of
   the three runs on a push, and nothing schedules them yet. Not built: the
   monkey's fed pointer reaches the viewport and the keys but no widget,
   because `input::set_mouse_down` feeds the input snapshot and egui reads
   the window's own events. It should also `ui::click` a random widget, once
   a link, a native file dialog and an export are held back in such a run.
3. **Pictures.** Built: `scripts/pictures.py` runs each example offscreen at
   a fixed tick and compares its 200th frame with the PNG in
   `scripts/pictures/`, on the Mac the release is cut on. A fault like
   defect 2's passes every other gate. A shader's `time()` reads `engine::time()`, so
   `shaders` draws the same frame each run too.
4. **The first app.** Built: `scripts/e2e.sh` checks and runs
   `../polyglot-pirates-app` when it is there, and the app's own
   `tests/taps.sh` presses every screen through its widgets with `ui::click`.
5. **By hand, before a tag.** What no script reaches: the pointer under a
   real mouse and a trackpad, an IME, a window resized and made full screen,
   a second display, the 14-inch window, an export opened on a real phone,
   on Windows and on Linux, sound by ear, and the first launch of the signed
   download. On a Linux desktop, a flip of the system's dark mode while the
   editor idles redraws it in the new look: `appearance::is_dark` wakes the
   loop, and no machine has run it yet.

## 6. The first app

Polyglot Pirates is seven scenes of a design mock in
`../polyglot-pirates-app`. It grows one feature of the website at a time.

1. It compiles (defect 1) and names the engine commit it builds on.
2. Each feature lands with a smoke scene of its own, as `round_smoke` is.
3. A thing the app needs and the engine lacks becomes a roadmap row, never a
   workaround in the app.

## 7. Not planned here

- **⌘Q on macOS asking first.** It leaves through AppKit's `terminate:`,
  which closes the windows without a close request, so `on_quit_requested`
  does not run. Routing it needs AppKit bindings beside winit's own; winit
  offers only turning its menu off, which takes Hide and Quit with it.
- **A CI job with a GPU.** `PLAN-hardening.md` step 1 holds it; the
  pictures in §5 run on a developer's machine first.
- **Scene `bindings` and `states`,** the `state_machine` and the built-in
  focus controller stay in the engine: `ARCHITECTURE.md` "What stays native"
  lists them as product decisions.
