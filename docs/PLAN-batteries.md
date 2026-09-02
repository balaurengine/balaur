> **Status:** not started. Written 2026-09-02. The pieces every game writes
> for itself when the engine has none: a UI toolkit for menus, input
> actions, save games, localization, audio buses.

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

## Open questions

1. **Retained tree or egui for menus.** The `ui` module is immediate-mode
   and complete; `widget` is retained and scene-shaped. Menus that a
   designer edits want the second, so it is the one that grows. Both stay.
2. **Where rebindings live in a replay.** A replay records raw input; the
   binding table is loaded state and must be captured in the recording's
   header, like the RNG seed.
