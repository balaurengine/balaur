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
   doubles when it fills, and a quad placed before that kept its old
   position in it. `balaur_text` places a block again when the atlas grew
   under it, and `text3d` rebuilds a block built under a smaller atlas. Not
   checked: a bitmap font's page region, which reads as the same fault.
3. **`test:inspector_rows` depends on load.** Built: the test waited eight
   frames, and under load those outlast the 0.3 seconds after which the
   Inspector refills its whole form; it now waits on that deadline. The
   deadline itself is §4's first item.
4. **`examples/interface` does not hold its memory.** Built: the Balaur
   Script compiler never released a value it handed to the engine, on a
   property write and on a `str?`, a struct or a class passed to a host
   call. `interface` leaked 19 KB a frame through `widget_rows.items = items`
   and now holds 18 MB from 6,000 frames to 48,000.
5. **A self-test writes the user's own data.** Built: `BALAUR_DATA_DIR`
   names the per-user data folder of a run, and `scripts/e2e.sh`, `monkey.sh`
   and `soak.sh` point it under `target/`. Before it, the states rewrote
   `balaur-editor/editor.toml` under the real home folder.
6. **A scene of widgets opens to an empty viewport.** Built: the Scene
   workspace draws the world and no widget, so a scene holding widgets and
   nothing of the 2D or 3D world opens in the UI workspace. `test:every_node`
   checks which rows make such a scene.
7. **A run's picture is paler than its scene.** Built: the film is linear
   light and no authored colour was decoded on its way in, so the resolve
   encoded a value that was already encoded. `balaur_render::film` decodes
   each one where the backend hands it to kiss3d, and a `#3c4753` shape is
   `(60, 71, 83)` in a shot. The default `ambient_color` and the four scenes
   that write one are the same light, written as a colour. Text is decoded
   the same way. Not done: a shader's `[params]`, which are its own numbers;
   and the fork's resolve, a 2.2 power where a sampler decodes by the sRGB
   curve, so a texel of 60 is drawn 62.
8. **`balaur check` passes a scene it cannot load.** Built: `check` builds
   every scene file with the loader a run uses, its scripts held, and reports
   what the loader refuses as an error on the line that writes it, naming the
   node, the component and the property.
9. **No gate compiles the library's addons.** Built: `check` compiles and
   lints every `.bs` file under `addons/` that no scene reached, an addon's
   `editor/` plugins included, and reports each under its own name.
10. **`examples/benchmark` fails when the editor plays it.** Built: its
    script found its cameras by a path from the root, and a scene the editor
    plays sits under the editor's own. It reaches them from its own node.
11. **A game may call what only the editor has.** Built: a module the
    editor's binary declares is `editor_only()`, each function marked where it
    is declared; `dependency` keeps `list` and `of` unmarked. Only the
    editor's files may call one: the project `balaur edit` runs as the editor,
    and anything under a game's or an addon's `editor/` folder. Any other file
    fails in `balaur check`, in Play and the Problems list, and in an export,
    and so does a `use` of an editor file or a scene attaching one. `balaur
    run` says "the engine has no `debugger::scope`", since a runtime declares
    none of the editor's names. An export leaves the editor's plugins out.
    `check` compiles every script `ignore` keeps, attached or not, which found
    the plugin dock hearing the save addon's `saved` by name; the addon's own
    plugin does that now. The judgment costs 0.7 ms of a 300 ms warm boot.
12. **Exporting the editor warns.** Built: the export linked the library's
    materials against the editor's root, and the library is a project of its
    own, so seven `WARN` lines named shaders under `editor/shaders/`. A
    material inside a nested project is left to that project.
13. **Nine self-test states ran nowhere.** Built: `scripts/e2e.sh` runs
    `test:tools`, `test:lint`, `test:palette`, `test:panels`, `test:imports`,
    `test:deselect`, `test:viewport_clicks`, `test:gestures` and
    `test:state_machine`. Five checks had failed unseen: a completion carried
    no doc line, hover answered nothing on a method's own name,
    `test:imports` trapped, `test:gestures` read the 3D camera in a 2D scene,
    and the dock's "add state machine" wrote a property nothing has.
    `test:manager` stays out: it passes only when the binary sits in cargo's
    build folder, and e2e runs a copy.
14. **`balaur_cli`'s scene tests failed at random.** Built: they share the
    process's log buffer, and one test's `clear` wiped the error another was
    about to read. They hold one lock now.
15. **A dock panel opened empty.** Built: a panel heard `docks_changed`
    while still hidden and turned visible a frame later with nothing telling
    it, so the State machine panel stayed empty until something else moved.
    Each of the fifteen panels that fills on show now also refills from its
    own node's `visibility_changed`. `test:state_machine` on `rig` caught it.
16. **A recorded session of `rig` did not replay.** Built: the frame a
    recording starts in is not recorded, and a replay holds the frame it
    loads in, but the recording went on stepping the game through the rest
    of its frame. `rig`'s state machine entered `wave` there, so every replay
    ran one step behind from its first tick. `start_recording` now holds the
    game until the next frame. A tween's handle came from a counter the
    editor's docks share, so the tween `fall.bs` starts on tick 207 took 1
    recorded and 3 replayed; a tool's tweens now count apart, and the header
    restores the game's counter. `test:recordings` runs on every example the
    states run on, where it ran on `angrynerds` alone.
17. **`test:timings` compared two frames.** Built: the window's period was
    noted after the frame advanced, so a script read one frame's stages
    against the period of the frame before, and a slow frame failed the
    check. The period is noted first now, and `TimingLog` pairs it with the
    frame it measured.
18. **A debug build can panic in a window that cannot present.** Not fixed:
    with the display asleep, the replay half of `test:recordings` hit egui's
    check that an id keeps its layer, for the tooltip id `balaur-tip` in
    `balaur_ui::widget::layer::tip`, which hashes the node and not the layer.
    It needs a run that reproduces it before a fix; a release build has no
    such check.

## 3. The API

Decided 2026-11-13. Each item breaks scripts, so each lands with the editor,
the library, the examples and Polyglot Pirates changed in the same commit.

1. **One way to hear an event.** A script hears an event in its
   `on_<name>(payload, source)` method: its own node's unasked, another
   node's after `events::listen`. A scene sends one to a script with a
   binding row. The widget's `on_click = "method"` keys go, `click` becomes
   `pointer_click`, and the names take `docs/NAMING.md`'s forms. A script
   declares the events its node sends, and `balaur check` warns on a handler
   or a listener naming an event nothing announces. `PLAN-events.md` §6 holds
   the design. Built: the handler shape, the `call` row that climbs to
   its method, the widget's keys gone with every scene's turned into rows,
   the pool keeping its specs' handler names, `pointer_click` for a widget's
   click, the `event` item, the check's warnings, the hooks page's
   listener-only events, and every name in `docs/NAMING.md`'s form.
2. **Editor leftovers leave the kernel.** Built: presets are the editor's
   `presets.toml`, which the picker builds with `set_component`, and a
   game's `editor/` and each addon's add their own. `dependency::install`,
   `add_path`, `make_local` and `move_into` are `balaur_cli`'s, beside
   `project` and `forge`. `balaur::ToolsPlugin` holds `debugger` and the
   `script::` tool verbs, which the editor's binary registers and no game's
   runtime does, and `script::store` is `engine::store`.
   `PLAN-engine-core.md` lists each.
3. **The demo plugin leaves the editor.** Built: the Counter dock's plugin
   is `scripts/plugins/counter.bs`, which `scripts/e2e.sh` copies into the
   project it runs `test:plugin_docks` on.
4. **A handle method does not repeat its component.** Built:
   `softbody2d.area()`, `joint2d.force()`, `vehicle3d.speed()`.
   `balaur_script::handle_method` names every handle's methods, and one that
   would spell a property takes a noun of its own, as
   `collider2d.total_mass()` does. A soft body sleeps and wakes with the
   body's own `is_sleeping` and `wake_up`.
5. **Vectors in, vectors out.** Built: a function that takes a point, an
   offset, a size, a direction or a velocity takes a `Vec2` or a `Vec3`, and
   one that takes a colour takes a `Color`: `set_linear_velocity(Vec2)`,
   `node.set_tint(Color)`, every `render::draw_*`. An answer that was a flat
   list is a record of vectors: a collider's `aabb()` holds `min` and `max`,
   `render::mouse_ray()` an `origin` and a `direction`, `camera_pose()` an
   `eye` and a `target`, `camera_2d()` a `center` and a `zoom`, and a
   character's move the offset it `applied`.
6. **`camera3d` aims by its node's rotation,** as `light3d` does. Built:
   the `look_at` and `up` properties are gone, and `transform.look_at(point)`
   turns any node to face a world point. `focus_distance` is how far ahead an
   orthographic frame with no height is sized and a stereo pair converges.
7. **3D immediate shapes say so.** Built: `draw_box_3d`, `draw_sphere_3d`,
   `draw_capsule_3d` and `draw_lines_3d`, beside `draw_rect_2d`.
   `draw_box_3d` takes the box's whole `size`, as `draw_rect_2d` does, and
   `draw_lines_3d` a list of `Line3d` records.
8. **Fed input is a setter beside its reader.** Built: `input::feed_mouse`
   is `set_mouse_position`, and the family follows: `set_mouse_down`,
   `set_key_down`, `set_scroll_delta`, `set_action_value`, `set_touch` and
   `set_gamepad`, each taking a `Vec2` where it took two floats.
9. **Dead surface goes.** Built for what an event already says:
   `scene::added_since`, `changed_since` and `removed_since` with
   `engine::change_tick` (the `changed` event), `input::dropped_files`
   (`files_dropped`) and `settings::revision` (`on_setting_changed`). Built
   too for what nothing called: `multimesh2d.set_instance_region`, which drew
   nothing, and `render::set_shader_probe` with the `shader_probe` that read
   its answer. Kept, each the only reach a script has: the
   `ui::focus_*` verbs, `engine::environment`, and `node.set_property`, which
   is what `node.component.x = v` compiles to. The focus verbs have no test.
10. **An example plays sound.** Built: `examples/sound` loops a positional
    hum, rings a bell from a `timer` and counts its `sound_finished`, and
    plays a thud with `audio::play_file` when a ball lands. `scripts/make_sounds.py`
    writes its three WAV files.
11. **`docs/NAMING.md` says why.** Built: four principles open the file,
    and every rule sits under the one it follows from. They are the reader's
    words (Godot, then Unity, then a domain's standard, then plain English),
    one word for one meaning, one shape per kind of thing, and a name that
    says what the call site cannot. An event names what happened, in the past
    tense, and input keeps Unity's pairs. The names that came from a backend
    took the reader's word: `point_size`, `stereo_separation`, `tab_size`,
    `specular_shininess`, `normal_scale`, `torn`, and a character's `exclude`.

## 4. What still polls

Every engine thread sleeps on its event. What is left reads a value each
frame to see whether it moved.

1. **A sleeping loop keeps time.** Built: before it sleeps, a
   `low_processor` loop asks `balaur_core::rest`, where each subsystem
   registers one probe. Frames keep coming while a `task` wait or a `timer`
   counts, a clip or a tween plays, a body is awake, particles live or a
   sound is due to end. Still to go: the editor's autosave, lint sweep,
   inspector refill and import expiry compare a clock, and can wait on
   `task::seconds` instead.
2. **The editor's file cache.** `model.bs` keeps `fs::read` and `fs::list`
   for half a second. `engine::watch` and `files_changed` forget an entry
   when its file moves.
3. **The recordings list.** `session.bs` lists a folder every half second
   and reads `replay::state` every frame. The first is `files_changed`; the
   second wants an event `replay` does not send yet.
4. **Transforms and the scene mirror.** `propagate_transforms` walks the
   whole tree and the mirror compares every renderable, every frame. Both
   read `balaur_core::changes`.
5. **Freed nodes.** Physics and audio find them with `retain(world.contains)`.
   `scene::announce_leaving` already holds the list.
6. **The device.** Dark mode, the safe area and the text scale are read and
   compared each frame. winit's `ThemeChanged` and the browser's `matchMedia`
   and `visualViewport` listeners say when they move.
7. **Pads on Windows.** gilrs enumerates controllers every 8 ms for the life
   of the process. The pad thread opens on the first pad read.
8. **A browser tab at rest.** A `low_processor` page wakes every animation
   frame to read a flag. A promise that input or `wake` resolves replaces it.
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
   `scripts/pictures/`, on the Mac the release is cut on. Defect 2 passes
   every other gate. A shader's `time()` reads `engine::time()`, so
   `shaders` draws the same frame each run too.
4. **The first app.** Built: `scripts/e2e.sh` checks and runs
   `../polyglot-pirates-app` when it is there, and the app's own
   `tests/taps.sh` presses every screen through its widgets with `ui::click`.
5. **By hand, before a tag.** What no script reaches: the pointer under a
   real mouse and a trackpad, an IME, a window resized and made full screen,
   a second display, the 14-inch window, an export opened on a real phone,
   on Windows and on Linux, sound by ear, and the first launch of the signed
   download.

## 6. The first app

Polyglot Pirates is seven scenes of a design mock in
`../polyglot-pirates-app`. It grows one feature of the website at a time.

1. It compiles again (defect 1) and names the engine commit it builds on.
2. Each feature lands with a smoke scene of its own, as `round_smoke` is.
3. A thing the app needs and the engine lacks becomes a roadmap row, never a
   workaround in the app.

## 7. Not planned here

- **A CI job with a GPU.** `PLAN-hardening.md` phase 4 holds it; the
  pictures in §5 run on a developer's machine first.
- **Scene `bindings` and `states`,** the `state_machine` and the built-in
  focus controller stay in the engine: `PLAN-engine-core.md` lists them as
  product decisions.
