> **Status:** phases 1-5 and 7 built; phase 6 is demand-driven by design.
> Phase 7 (positional audio) on 2026-09-04:
> `crates/balaur_audio/src/spatial.rs`, the `listener` component, `positional`
> with `min_distance` / `max_distance` / `doppler` on `sound`, a `position` in
> `audio.play` and `audio.play_event`, and `audio.listener` /
> `set_listener` / `emitter_position` / `set_emitter_position` /
> `distance_gain` / `pan`. Phase 2
> (audio buses and events) on 2026-09-03: `crates/balaur_audio/src/bus.rs`
> and `event.rs`, `[audio.buses]`, `audio/events.toml`, a `bus` on the `sound`
> component and in `audio.play`'s options, and `set_bus_volume` re-applying to
> what is already playing. Phase 5 (localization) on
> 2026-09-03: `crates/balaur_core/src/strings.rs`, the `strings` module,
> `[locale]` in `project.toml`, plural rules for the shapes a translator
> actually writes, `widget.text_key`, and `examples/hello` in English and
> Romanian. Phase 4 on 2026-09-03 — `row`,
> `column` and a `panel` that lays out, with `padding`, `gap` and `align`;
> focus with keyboard navigation, `focusable`, `on_focus` and the `ui.focus_*`
> verbs a pad reaches it through; and the `widget_theme` asset, inherited down
> the widget tree. `examples/hello` gains a themed menu that drives the
> spinner. Phases 2 (audio buses), 5 (localization) and 6 (more widget kinds)
> are not started. Phase 3 (save games) on 2026-09-03:
> `crates/balaur_core/src/save.rs`, the `save` script module, `[save]` in
> `project.toml`, and migrations run one version step at a time. Phase 1
> (input actions) on 2026-09-02 —
> `crates/balaur_input/src/actions.rs`, the `[input.actions]` table, the
> `input.action_*` calls, rebinding saved to the user data directory, and a
> test that the same keys produce the same actions every run. Phases 2-6
> (audio buses, save games, the widget tree, localization) are not started.
>
> **Where the implementation decided differently:**
>
> 1. **Bindings are spelled the way the constants are.** The sketch wrote
>    `"gamepad:a"`, `"axis:left_x"`; the engine already names these things
>    `input.PAD_SOUTH` / `"South"` and `input.AXIS_LEFT_STICK_X` /
>    `"LeftStickX"`, and a binding string that disagreed with the constant
>    for the same button would be a second vocabulary to learn. So:
>    `"Space"`, `"mouse:left"`, `"gamepad:South"`, `"axis:LeftStickX"`,
>    `"keys:A,D"`.
> 2. **Half-axes exist.** `"axis:LeftStickY+"` binds one direction of a
>    stick, because without it a stick cannot drive an action that is only
>    ever pressed or not.
> 3. **Actions read every pad, not one.** A single-player game does not care
>    which controller a value came from, and per-pad actions can be added
>    without moving anything.
> 4. **Edges compare values, not bindings.** Keeping last frame's value per
>    action is what lets a stick pushed past the threshold fire
>    `just_pressed` the way a key does. It is derived from the recorded raw
>    input each frame, so a replay reproduces it.
> 5. **Open question 2 is closed, with a seam rather than a field.** The
>    recording's header carries the binding table and restores it before the
>    first tick, so rebinding after a session cannot change what it replays.
>    Rather than teaching core the word "bindings", `App::add_replay_setup`
>    is a once-at-start twin of `add_replay_source` for anything a plugin
>    loads rather than simulates — a locale and an audio mix will want it
>    too. A recording made before a plugin declared its setup still plays.
> 6. **No `taffy`.** The plan names it for the arithmetic; egui already does
>    row, column and grid layout with spacing and margins, it is what the
>    editor's own panels use, and the renderer is egui either way. A layout
>    crate earns its place at wrapping and percentages, which nothing has
>    asked for. Layout is presentation, so neither choice reaches the digest.
> 7. **A container's parent is the nearest *widget* ancestor.** Not the
>    parent node: a menu is usually a panel with a grouping node or two
>    inside it, and the layout should not care.
> 8. **`panel` lays out too.** The plan lists containers as new kinds beside
>    the existing ones; a panel is already a frame, and a frame with things
>    in it is what a menu is made of. One with no children is unchanged,
>    which every existing scene depends on.
> 9. **Focusability is derived, not declared.** The plan gives focus its own
>     flag; a flag alone would let focus land on a label, where an accept has
>     nothing to do. So a stop is a widget that can be activated, and the flag
>     can only remove one.
> 10. **The keyboard is egui's, the pad is the assembly's.** `balaur_ui`
>     already takes its input from egui, so keys cost nothing. Pads live in
>     `balaur_input`, which knows nothing about widgets, so `standard_app` —
>     the crate that knows about both — maps `ui_next` / `ui_previous` /
>     `ui_accept` onto the focus verbs. Neither plugin grew a dependency.
> 11. **A theme owns chrome, not content.** The plan says "fonts, colours,
>     nine-slice panels, per widget kind". Colours and geometry per kind is
>     what landed; text and size stayed the widget's. That division is what
>     avoided the real problem — every widget property has a schema default,
>     so there is no "unset" for a theme to fill in, and a theme that could
>     set `text_color` would have no way to know whether the author meant the
>     default or merely accepted it. Nine-slice waits for an image-backed
>     panel to exist.
> 12. **Plural rules are a named handful, not CLDR.** The plan says "by CLDR
>     rules"; the full table is a generated artefact of some size, and what a
>     game needs is the shapes its own translators write in. English,
>     Romanian, the Slavic one, French and the one-form languages are named;
>     everything else gets the English rule, which is also the right rule for
>     most of the table. Widening it is adding a match arm.
> 13. **A missing key comes back as itself.** Not in the plan. A blank label
>     is a bug that hides and a key on screen is a bug that reports itself.
> 14. **One fallback hop, not a chain.** Two means two files to hold in mind,
>     and the second is always the language the game was written in.
> 15. **Variations rotate rather than shuffle.** The plan says "a named
>     sound with variations"; how one is picked was open. A rotation never
>     repeats a sample back to back — which is the whole reason to record
>     three of a footstep — and it keeps what a player hears out of the
>     engine's random stream, where a muted game and a loud one would draw
>     differently and diverge.
> 16. **Ducking is not built.** The plan lists it with buses. It needs a bus
>     to know when another is sounding, which is a subscription the mixer does
>     not have yet; the volume tree is the part a game asks for first.
> 17. **A migration is a project-declared script, not a per-node hook.** The
>    plan says "a script declares `migrate_save`", which leaves open *which*
>    script — a game has many, and a save belongs to none of them.
>    `[save] migrate = "scripts/saves.rn"` names one, and the engine calls it
>    per version step. That needed `ScriptHost::call_in(path, function,
>    args)`: a public function in a file, with no instance. The seam is
>    general, and the next project-level hook will want it too.
> 7. **A newer save is refused, not migrated.** Not in the plan. Migrations
>    only go forward, so a file from a build ahead of this one has no path
>    back; saying so beats handing the game a table it will misread.
> 8. **Writes are atomic.** Also not in the plan, and the reason is the same
>    one that makes saves worth having: the file written at a checkpoint is
>    the one a crash must not be able to truncate.
> 9. **`ManifestSource` had to exist.** A plugin reading its own table out of
>    `project.toml` through `ProjectFiles` finds nothing in a packed game —
>    a pack carries the manifest beside the assets, not among them. The raw
>    manifest text is now an engine resource, which is also the bug
>    `NetConfig` still has.
> 18. **The listener is its own component, not the camera.** `balaur_audio`
>    depends on neither `balaur_render` nor a camera, and a 2D game whose
>    view leads the player wants the ears where the player is. A game with no
>    node for it calls `audio.set_listener`; a game with neither hears every
>    sound flat, which is the only failure mode that keeps a game audible.
> 19. **rodio's `Spatial` is not what plays a positional sound.** It folds
>    distance into the pan through a fixed inverse square and a pair of ear
>    positions, so a game cannot say what its unit is. `ChannelVolume` under
>    the engine's own placement gives the same two channels with the numbers
>    the scene chose — and positioning a sound makes it mono, because a sound
>    in one place has one direction.
> 20. **Doppler is off unless a sound asks for it, and is clamped to an
>    octave.** The plan lists it beside attenuation; physical doppler on a
>    position that jitters is a warble, and on one that teleports a screech.
>    So `doppler = 0` by default, `1` is physical, and the bend stops at
>    double and half whatever the arithmetic says.
> 21. **A placement is remembered, not read back off a sink.** The rule the
>    rest of the crate holds to: `AudioState::placement_of` is what a run
>    with no output device asserts, and `audio.distance_gain` / `audio.pan`
>    are the same numbers for a debug overlay.

# Plan: batteries

## Where the tree is today

- UI: `widget` components (label, button, panel) anchored to the screen,
  and the immediate-mode `ui` module scripts and the editor draw with.
  No layout tree, no theme asset, no gamepad focus.
- Input: a per-frame snapshot of keys, mouse, gamepads and touch, read by
  constant. No named actions, no rebinding.
- Saves: `engine::user_data_dir()` and `fs`; a game serialises its own
  tables. No versioning.
- Localization: none.
- Audio: `audio::play` with volume and looping per call. No buses, no
  events.

## Game UI toolkit

The `widget` component grows into a tree rather than a second system.
Three additions: `container` widgets that lay out their children (row,
column, grid, with padding and gaps — `taffy` does the arithmetic), a
`theme` asset (fonts, colours, nine-slice panels, per widget kind, by
reference or inline like every asset), and focus: one focused widget per
screen, moved by gamepad and keys in reading order or by an explicit
`focus_next`, with `on_focus` and `on_activate` events named like
`on_click` is. New widget kinds as they are needed: `text_input`, `slider`,
`list`, `progress`. The scene file stays the description — a menu is
nodes — so the editor's Interface persona edits it as it edits anything.

## Input actions

An `[input]` table in `project.toml`:

```toml
[input.actions]
jump = ["Space", "gamepad:a"]
move_x = ["axis:left_x", "keys:a,d"]
```

Scripts ask by name — `input::action_pressed("jump")`,
`input::action_value("move_x")` — and the raw snapshot stays underneath,
so a replay records keys and derives actions deterministically. Rebinding
is `input::bind("jump", "gamepad:x")`, saved to the user data directory and
loaded at boot. The editor lists actions in the project settings.

## Save games

A `save` module over the scene serializer: `save::write(slot, table)` and
`save::read(slot)` write TOML with a `version` into the user data
directory. A script declares `migrate_save(version, data)` and the loader
calls it once per version step, so an old file always arrives at the
current shape. Nothing here is engine state; a save is whatever the game
puts in the table.

## Localization

Strings live in `strings/<locale>.toml`; `tr("menu.play")` reads the
current locale with a fallback chain, and `tr("items", #{ n: 3 })` picks a
plural form by CLDR rules. `engine::set_locale` switches at run time and
widgets showing a key redraw. Fluent syntax is the second step if TOML
tables prove too flat.

## Audio buses

An `[audio.buses]` table declares buses with a volume and a parent
(`sfx → master`); `audio::play(sound, #{ bus: "sfx" })` routes to one, and
`audio::set_bus_volume` fades it. An event is a named sound with variations
and a bus, declared in `audio/events.toml`, played by name, so a game tunes
its sound without touching scripts. Ducking is one bus lowering another
while it plays.

## Phases

1. Input actions and rebinding; a test that a replay reproduces actions
   from keys.
2. Audio buses and events.
3. Save games with versioning and migration.
4. Containers, the theme asset and focus for `widget`; the Interface
   persona edits them.
5. Localization with plural rules.
6. The remaining widget kinds as games ask for them.
7. Positional audio: distance attenuation, stereo panning and doppler off a
   listener node, over the buses that already exist.

## Open questions

1. **Retained tree or egui for menus.** The `ui` module is immediate-mode
   and complete; `widget` is retained and scene-shaped. Menus that a
   designer edits want the second, so it is the one that grows. Both stay.
2. **Where rebindings live in a replay.** A replay records raw input; the
   binding table is loaded state and must be captured in the recording's
   header, like the RNG seed.
