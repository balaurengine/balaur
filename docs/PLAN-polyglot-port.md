> **Status:** written 2026-09-24 from the port at `ca47fb3` and the game at
> `483b898c1`, three commits past the last reimport. Steps 1 and 2 landed
> the same day, and step 3 in part; §0 carries the counts before and after.
> §0 is what was measured, §1 what is missing and where each piece belongs,
> §2 the order, §3 the estimate and its basis. `docs/PLAN-godot-import.md`
> holds the importer and `docs/PLAN-gdscript.md` the translator; this plan
> holds what is left between them and a game that plays.

# Plan: finishing the Polyglot Pirates port

`balaur import` converts the game whole and the port passes every automation
scenario Godot passes. What the automation does not reach is what is left:
the input handlers the translator leaves as comments, the drawing a node does
by hand, the online codec, the theme's states, and the three targets the game
ships on that the port has never been exported to.

## 0. Where it stands

Measured on 2026-09-21 unless a date says otherwise.

- The port passes 32 of the game's 54 automation scenarios; Godot passes 30.
  The 22 Godot fails are its online and guest flows. The automation has
  stopped finding gaps on its own, so this plan works from the report and
  the stubs instead.
- Step 1's run on 2026-09-24, at the engine's head and the game's, with a
  release build and a load average near 300 from a peer's test run: 27 pass,
  20 fail where Godot fails the same 20, and the 7 `settings_*` scenarios
  that passed on the 21st hit the 300 s watchdog. The scenario logs also
  carry the runtime errors the stubs hide: `name.hash()` alone fired 5 184
  times a run, and five sites called a signal handler with the wrong count.
  `port/automation/run.sh` never sees them: its ` ERROR ` grep misses the
  colour codes the log carries, so a scenario passes with any number of
  script errors in it. Three offscreen pictures from that run: the main
  menu draws its ship, sea and bottom bar; the world map is a flat brown
  texture with nothing on it; a practice game never leaves `STARTING`, as
  in Godot.
- Four game files are still hand-written: `scripts/controller/controllers.rn`,
  `scripts/ui/intro_menu.rn`, `scripts/ui/common/panel_show_button.rn` and
  `scripts/animation/initial_animation.rn`. The Gamend facade is 623 lines
  in `port/godot_*.rn` behind a 211-line generator, and stays by design
  (`docs/PLAN-gamend-bindings.md` step 6).
- The translated code holds 2 522 `(gd.todo)` stubs over 338 names. Each one
  logs an error when it runs. 1 859 sit in `addons/polyglot/proto`, about
  450 in the game's own scripts and scenes, 211 in addons no scene uses.
  After steps 2 and 3 on 2026-09-24: 709 everywhere, 198 in game-owned code
  over 113 names, 2 in `addons/polyglot/proto`; the reimport writes 866
  scripts, the 97 new ones being nested inner classes, and `balaur check`
  reports no problems. After steps 4 to 7 the same day: 649 everywhere, 154
  in game-owned code over 100 names, the theme's unmapped items 280 to 180,
  and `login_offline` logs 41 error lines, 33 of them the cutout shadow's
  `_draw` meeting a member the bake never set.
- The report lists 6 969 notes; 2 261 are in game-owned code, of which 1 280
  are a missing value, 652 a missing call and 329 everything else.
- The port has never been exported. The game ships Web, iOS, Android, macOS,
  Windows and Linux.
- The last picture of the port is from 2026-09-12, and showed mostly empty
  panels. A release build has not been timed on the 5 278-node main scene.

## 1. What is missing

### 1.1 Engine

Input events first. 43 game scripts have an `_input`, `_unhandled_input`,
`_gui_input` or `_unhandled_key_input`; the bodies measure 518 lines over 40
functions, with 61 `set_input_as_handled` and 13 `set_process_input` sites.
The engine polls `input` from `update` and delivers nothing, so the
translator keeps every handler as a comment (`docs/PLAN-gdscript.md` §1).
Ship steering, world-map pan and zoom, the camera, every popup's escape,
`focus_manager` and `click_gesture_area` are behind this one gap.

What the game reads from an event: the class (`InputEventMouseButton` 29
sites, `InputEventScreenTouch` 21, `InputEventKey` 9, `InputEventScreenDrag`
8, `InputEventMouseMotion` 8, pan and magnify gestures 5 each), the
position, the button or key, `pressed`, and `is_action_pressed` (36 sites).

The engine already delivers events, in its own shape
(`crates/balaur/src/interact.rs`): `on_pointer_down(this, button)`,
`on_pointer_up`, `on_pointer_click`, `on_pointer_drag`, `on_pointer_enter`
and `on_pointer_exit` reach the node under the pointer, and
`on_key_down(this, key)`, `on_key_up`, `on_action(this, name)`, `on_scroll`
and `on_resize` reach every node, bindings first and the script second. A
finger is a left click through `emulate_mouse_from_touch`; pinch, pan and
swipe are read from `input`. Two things are missing, and both stay in that
shape: a hook that returns `true` has handled the event and the broadcast
stops there, which is what `set_input_as_handled` becomes; and
`pointer_down` and `pointer_up` reach every node when the pointer is over
none, as `scroll` already does, so a panel can close on a click outside it.

The translator carries the rest. An `_input` body becomes a private
`__input(this, event)`; each event class the body tests for
(`InputEventKey`, `InputEventMouseButton`, `InputEventScreenTouch`) is
emitted as the `on_key_down`, `on_pointer_down` or `on_pointer_up` that
builds the event table and calls it, and `event.is_action_pressed("x")`
reads `input.action_just_pressed`. `_unhandled_input` is the same hooks
behind `ui.wants_pointer()` and `ui.wants_keyboard()`, the widget layer's
own claim. `set_process_input(false)` is a flag the hooks open by reading,
like `process_enabled`. Pan and magnify gestures, and mouse motion, are
polled from `update`.

Node-owned drawing is the second gap. 8 scripts define `_draw` (59 lines of bodies) and
call `queue_redraw` from 27 sites: the hint and anchored connectors (curves
and circles), life pips, city graph lines, camera bounds, and
`cutout_shadow_2d.gd`, 673 lines used by 17 scenes. `render.draw_*_2d` is
one frame, over everything, with no node transform and no `z_index`.

What the immediate API lacks is a place in the order: a transient shape is
added last (`draw_2d.rs::flush`), and a texture has no region. So each
`render.draw_*_2d` takes an optional trailing table, `#{ z: 3 }` on any of
them and `region` on the texture, and the transient takes its place in the
`z_index` order `sync_2d.rs` keeps. The translator emits `_draw` as a
private `__draw(this)` the script runs from `update` while the flag
`queue_redraw` raises is up, with the node's global transform applied by
the shim before each verb. The cutout shadow's bake through a `SubViewport`
waits on the roadmap's "More than one view" (0.3); until then it draws its
polygon pass only.

The theme is the third. 280 of the `shared_menus` theme's 474 items have no
`widget_theme` key: 107 colours, 53 constants, 51 icons, 66 styles, one font
and one size. The theme has `rest`, `hover` and `active` tables; Godot's
`disabled` and `focus` states, and the icons a kind draws (check, spin
arrows, tab close, slider grabber, dropdown and fold arrows), have nowhere
to land. The first step measures the 280 by key name; the report only
counts them by group.

The rest are small keys, each a few sites and its own row:

- **Cursor shape** — 27 `Control.CURSOR_*` sites; a `cursor` key on the
  widget, over the `egui::CursorIcon` `table.rs` already sets.
- **`mouse_filter`** — 15 exports dropped; a widget key that lets the
  pointer pass through.
- **Focus neighbours** — 2 sites of `focus_neighbor_*`.
- **App lifecycle** — `NOTIFICATION_APPLICATION_FOCUS_IN`, `_OUT`,
  `_PAUSED` and `WM_CLOSE_REQUEST`, 8 sites; the desktop half of the
  roadmap's "Suspend and resume" (0.8).
- **A texture from bytes** — 2 sites of `ImageTexture.create_from_image`.
- **Signals** — `tab_changed` (2), `gui_input` and `text_change_rejected`
  connections nothing emits.
- **Tracks** — `z_index`, `scale` on a Control and `update_position`, 16.
- **Nodes** — `MultiMeshInstance2D` (3) as `polygon` children sharing one
  mesh, `AnimatedSprite2D` (1) onto `sprite_sheet`, a particle's colour
  curve (6).
- **Regular expressions** — 3 `RegEx.new()` sites in the pinyin index; a
  `regex` module over `regex-lite`, the whole surface (`compile`, `search`,
  `search_all`, `replace`, `split`, `escape`).

### 1.2 Translator and importer

One translator rule accounts for 1 859 stubs, an inner class reading the
outer's constants: `polyglot_hook_pb.gd` (8 177 generated lines) defines `PB_SERVICE_STATE`,
`PB_DATA_TYPE`, `DEFAULT_VALUES_3`, `PB_ERR` and `PB_RULE` at the top, and
its 39 inner classes read them 1 200 times. `emit.rs` writes each inner class
to its own module (`inner: BTreeMap`) and gives it none of the outer's
items. The fix is what §4 of the translator plan does for a base class: copy
the outer's `const` and `enum` items into the inner module. The game's
metadata, lobby and key-value codecs go through this file, so every online
scenario depends on it.

The next group is calls onto modules the engine has. `map.rs` holds 108
arms; these names still fall to `gd.todo`:

| Godot | Sites | Engine |
| --- | --: | --- |
| `Performance.get_monitor` | 19 | `engine.timings`, `render.stats` |
| `JavaScriptBridge.eval`, `get_interface` | 13 | `web.visible`, `web.location`; the heap probes answer nothing |
| `FileAccess.open`, `get_sha256`, `get_file_as_bytes`; `DirAccess.rename_absolute`, `remove_absolute` | 14 | `fs`, `hash` |
| physics ray and point queries, `get_world_2d` | 15 | `physics2d.raycast`, `point_hits` |
| `WebSocketPeer` | 10 | `websocket` |
| `AudioServer.set_bus_volume_db`, `set_bus_mute` | 10 | `audio.set_bus_volume` |
| `get_viewport_transform`, `get_global_transform_with_canvas`, `to_local` | 16 | `render.camera_2d`, `transform` |
| `Geometry2D` | 6 | `geometry2d` |
| `HTTPRequest` | 4 | `http` |
| `DisplayServer.screen_set_keep_on`, `virtual_keyboard_get_height` | 5 | `window.set_keep_awake`, `input.keyboard_height` |
| `quit()`, `process_frame`, `create_timer`, `get_child_count` | 25 | `engine.quit`, `task.frames`, `task.seconds`, `node.children` |
| `Marshalls`, `Vector2.from_angle`, `char`, `String()` | 37 | `encoding`, `math`, the shim |

Constants are the other half of the missing values: `Control.CURSOR_*`
(27), `BoxContainer.ALIGNMENT_*` (7), `OK` (12), `TYPE_*` (9), `Color.*`
(13), `KEY_*` and `NOTIFICATION_*`. One table in `map.rs` carries them.

Members the resolver misses are the rest: `fade_tween` (19), `velocity`
(7), `polygon` (7), `texture` (6), `skeleton` (5), `original_scale` (3).
The node ones are class properties the map does not know
(`CharacterBody2D.velocity`, `Polygon2D.polygon`); the others are
diagnosed one name at a time.

Three rules belong to the importer rather than the translator:

- **A material on a `ColorRect` under a `Node2D`** — 32 of the 34 "shader
  on a Control" notes are `FoamTrail` rects inside creature and obstacle
  scenes, with no Control above them. They become a `sprite` carrying the
  material. The two real ones, `DarkModeTwilight` and `AnimatedPattern`, are
  ported by hand as world-space quads on a high `z_index`.
- **Export types** — 15 `MouseFilter`, 6 each of `PlayerHats`,
  `PlayerAccessory` and `PlayerColor`, 3 `PerspectiveProfile`, and 44 whose
  type the reader left blank. An enum-typed export is an `enum` with
  `options`; a typed array or dictionary is a `list` or `map`
  (`docs/PLAN-property-types.md`).
- **Autoloads** — `ThemeEvents` and `PerfProbeRunner` become the first two
  nodes under the main scene's root.

### 1.3 The port repository

- Reimport at the game's head; it moved three commits (quests, naming).
- Retire the four hand-written files as the steps above land. Each carries
  the reason it exists in its header comment.
- Export for the web and run `scripts/web_smoke.mjs` over it. The project is
  297 MB with its history and holds 2 824 SVG rasters, so the pack's size is
  the first number to take. The 13 `JavaScriptBridge` sites map onto `web`.
- iOS and Android: `balaur export` writes both, unsigned, and no frame has
  been rendered on a device (`docs/PLAN-mobile-export.md`).
- A picture per scenario against Godot's own store screenshots
  (`scripts/automation/generate_store_screenshots.sh`), and a frame time on
  the release build.

### 1.4 Not planned

- **The Discord SDK** — 91 stubs, for an activity target the game's
  `export_presets.cfg` does not name.
- **`softbody2d`, `DropShadowCaster2D`, `rmsmartshape`** — no game scene
  uses them; the shadow addon's stubs are in its examples.
- **Editor addons** — SimpleTODO, AutomationTestRunner, the csv importer,
  the polygon triangulator and the deep-link export plugin run in Godot's
  editor and have no counterpart to need.
- **The 35 `Callable` exports** — editor tool buttons.
- **Inbound deep links** — the plugin's export half is editor-only, and the
  runtime half has no plan on the engine side.
- **The 22 scenarios Godot fails** — the port is judged per scenario
  against Godot's result (`docs/PLAN-gdscript.md` §11), not against all
  pass.

## 2. Steps

1. **Measure again.** Release build at the engine's head, reimport at the
   game's head, all 54 scenarios under a watchdog, Godot's result for every
   port failure, and three offscreen pictures. Write the counts into §0.
   Done 2026-09-24; the pictures wait for a quiet machine.
2. **Inner classes.** Done 2026-09-24: an inner class carries the outer's
   constants and enums and its siblings as preloads, classes nested inside
   it recurse into files of their own, and `PB.Msg.Part` resolves through
   any name that reaches the file. `addons/polyglot/proto` fell from 1 859
   stubs to 2, the `StreamPeerBuffer` a double field packs through.
3. **The map.** The constants table, the calls in §1.2's table, the class
   properties. Reimport; the stub count in game-owned code is the check.
   Half done 2026-09-24: about 40 rows, plus four rules the scenario logs
   asked for. A `#` inside a string opened a comment, so a member holding
   `Color("#ff8a7a")` hid every declaration after it; a foreign node's method
   handed to `connect` was read, which called it, and is bound now; a
   handler of another class's signal takes its own count and the shim fits
   the payload to it; `text[i]` on a `String` goes through the shim. On
   `login_offline` the log's runtime errors went from 125 lines to 9 and
   its stub lines from 12 492 to 2 204, at the same instruction count.
4. **Input events.** `handled` and the pointer broadcast in `interact.rs`,
   each with a test in `crates/balaur/tests`; then the translator's `_input`
   rules and the `event is InputEvent…` tests. Every one of the 43 files
   reimports without a `PORT(gdscript)` comment, and `panel_show_button.rn`
   leaves `ported.txt`. Built 2026-09-24: a hook answering `true` ends the
   broadcast, a press reaches the node under the pointer and then every
   other node, and a class with an `_input` gains the six hooks that build
   the event table and hand it over. Mouse motion with no button held, and
   an action bound to a pad alone, reach no handler yet.
5. **`z` and `region` on `render.draw_*_2d`** and the `_draw` rules; the
   eight scripts reimport and draw. The rules landed 2026-09-24: a class
   with a `_draw` draws it every frame from `update`, and thirteen verbs go
   through the shim, which runs each point through the node's transform and
   the one `draw_set_transform` set, cuts a concave polygon into triangles,
   and flips Godot's arc angles. The two options on the engine's verbs are
   not built: a script's drawing sits over the scene, and a texture region
   draws as the whole picture.
6. **Importer rules**: the `FoamTrail` sprites, export types, autoloads.
   Done 2026-09-24: a `ColorRect` with a material under a `Node2D` is a
   `shape2d` rectangle carrying it, 34 notes to 1; an export typed by a class
   that extends a script by its path is a node, a Control enum an int, and
   an unknown hint with a preloaded default a path, 36 notes to 3; and the
   autoloads are the first nodes under the main scene's root, the scene
   found through its uid.
7. **Theme states and icons**, after a count by key name. The count, taken
   2026-09-24 over the 474 items: 164 colours, 98 constants, 129 styles, 51
   icons, 18 fonts and 14 sizes. Built the same day: a `widget_theme` kind
   takes `disabled` and `focus` tables, the importer fills all four states
   with Godot's styleboxes and its font and icon colours, and a
   `MarginContainer`'s four margins are one padding. The 51 icons, and the
   shadow, caret, selection and placeholder colours, are still reported.
8. **The small keys** of §1.1, each with its test; the `regex` module last.
   Built 2026-09-24 so far: `cursor` on a widget (`mouse_default_cursor_shape`
   from a scene or a script), and `pointer_through` (`mouse_filter` of
   `IGNORE`), which the widget layer reports so `ui.wants_pointer()` stays
   false over it while a button inside still takes the click. Found on the
   way: a press `input.feed_*` handed in was the frame's own and the pointer
   hooks run at the top of the next, so no fed press ever reached one; a fed
   event is now the next frame's, in a windowed and a headless run alike.
   Also built: a `regex` module in `balaur_core` over `regex-lite`
   (`matches`, `search`, `search_all`, `replace`, `split`, `escape`), which
   the shim's `RegEx` record calls; `Image.new()` + `load` as the texture's
   path. Not built, in balaur terms: `focus_neighbor_*` (the engine moves
   focus itself), the app lifecycle notifications (the roadmap's "Suspend
   and resume"), and `tab_changed`. A `MultiMesh` is built as `polygon`
   children of its node, each with the same inline `mesh` asset, so the
   batcher draws the city waves in one call per layer; `t.x = v` on a
   `Transform2D` local rebuilds it through `gd.with_field`.
9. **Retire the hand-written files** and reimport; the 54 again. Started
   2026-09-24 from the `login_offline` log, 1 394 error lines once
   `run.sh` read it stripped of colour: `Shader.new()` and
   `ShaderMaterial.new()` (510 each) are records the script keeps and the
   engine never draws with, `CircleShape2D.new()` (96) patches the node's
   `collider2d` when handed to a `shape`, `foldable_group` (185) and
   `material` are kept as meta, `add_theme_*_override` lands on the widget
   key it has (`text_color`, `gap`, `padding`, `font_size`) or nowhere,
   `get_popup()` is the button itself, a node's `duplicate()` clones its
   components and children, and `size_of` answers a vector. After the
   reimport (engine cb25b9b3): stubs 650 → 621, `login_offline` reaches its
   summary with 294 error lines, 185 of them `foldable_group` reads.
10. **Web export**, `web_smoke`, the size report, the `web` mappings.
11. **Pictures against Godot's**, and what they show fixed.
12. **A device.** iOS and Android signed and run once hardware is on the
    desk.

Each engine step lands with its `docs/ROADMAP.md` row and the plan it
belongs to (`PLAN-input.md`, `PLAN-2d-games.md`, `PLAN-widgets.md`).

## 3. Estimate

The basis is the port's own history: the importer landed on 2026-09-11, the
translator on 2026-09-12, the thirtieth scenario on 2026-09-19 and the
Gamend facade on 2026-09-21, ten working days for 769 files. A step below is
sized against that pace, in working days, and each is revised when its step
1 numbers come in.

| Step | Size measured | Days |
| --- | --- | --: |
| 1 Measure | 54 scenarios, one build, one reimport | 0.5 |
| 2 Inner classes | one rule, 1 859 stubs | 0.5 to 1 |
| 3 The map | about 150 names, about 450 stubs | 2 to 3 |
| 4 Input events | two rules in `interact.rs`, 518 lines of handlers in 43 files | 2 to 3 |
| 5 Node drawing | 8 scripts, two options on six verbs | 1 to 2 |
| 6 Importer rules | 32 sprites, 80 exports, 2 autoloads | 1 |
| 7 Theme | 280 items, two states, six icon keys | 2 to 3 |
| 8 Small keys | nine rows | 2 to 4 |
| 9 Retire and reimport | 4 files | 1 |
| 10 Web | one export, unknown size | 1 to 2 |
| 11 Pictures | open until step 1's pictures say | 3 to 5 |
| 12 Devices | needs hardware and signing | not counted |

That is 16 to 25 working days to a port that plays as Godot's does on
desktop and in a browser, with the online flows Godot itself passes. Steps
1 to 3 cost three to four days and remove about 2 300 of the 2 522 stubs;
step 4 is what ship steering, the map and every popup's escape wait on.

## 4. Open questions

1. **A widget over the world.** The 2D picker takes the smallest shape
   under the pointer and knows nothing of widgets (`pick.rs`), so a click on
   a button over the map reaches the map too. Whether `wants_pointer`
   should hold the pointer hooks back, or a game guards its own, is decided
   by the first scenario that hits it.
2. **The shadow bake.** Whether `cutout_shadow_2d`'s viewport bake is worth
   the wait on "More than one view", or the port ships the polygon pass.
3. **The theme's roles.** 12 of the theme's types are the game's own
   variations (`WordEntryLabelLockedBottomBad` and kin) with per-state
   colours; whether a `role` carries states of its own or the game's
   scripts set the colour is decided by the count in step 7.
