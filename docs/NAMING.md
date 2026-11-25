# Balaur naming

A name is the part of the engine people type, read and build on, and it costs
the most to change: a renamed function or key breaks every project that
spelled it. Four principles decide every name. Each says why it holds and what
follows from it. A name they leave open is under Decisions, with its reason.
Where `ARCHITECTURE.md`, a plan or the code disagrees with this file, this file
wins.

A number in brackets is the one a lint cites. The rules for Rust items inside
`crates/`, which only the engine's contributors meet, are in `CONTRIBUTING.md`.

## A. Use the words the reader already knows

A name is read far more often than it is written, and a word the reader has to
look up teaches them nothing. So who reads a name decides whose words it uses.

- A game developer reads the script API, scenes, the editor and the
  command line. Godot's word comes first, because Balaur opens Godot projects
  and its editor shows Godot's class names. Unity's comes next, when Godot has
  none or Godot's would break B. Then the domain's standard: W3C key codes,
  SDL3 pad names, glTF materials, CSS for layout and theme properties, Cargo
  for the command line. Plain English last.
- Nobody reads a backend's words: rapier, kiss3d, egui and cosmic-text are how
  the engine is built today. An option, a key or a feature is Balaur's
  word [N14].
- A word is spelled out, in US spelling: `string`, not `str`; `ui.slider`, not
  `hslider` [D4]. The editor shows the API's words in sentence case [N23], and
  no name in a script is one letter [N24].
- An event says what happened, in the past tense, as Godot's signals do:
  `finished`, `child_added`, `visibility_changed`. Pointer, key and contact
  events keep Unity's pairs, `pointer_enter`, `key_down`, `collision_enter`:
  Godot names those per node class, and Balaur has one name for every node
  [N22].

## B. One word, one meaning

A second word for one thing makes the reader ask how the two differ. One word
for two things makes them confuse the two.

- Within a scope a word means one thing; the glossary holds the words that
  could drift [N1].
- An API offers one way to do a thing. A new spelling replaces the old one; it
  never joins it.
- A word is spelled once, as a constant, so two spellings cannot drift: a
  crate's `vocabulary.rs`, a script's `const` [N17].
- A function has one shape, and two shapes are two functions. `any` is for
  what no one type is true of, each use with its reason [N25].
- Two components on one node that announce the same happening put their own
  name first, so one handler does not hear both: `sound_finished`,
  `particles_finished` [N27].

## C. One kind of thing, one shape of name

A reader who has learned one name can then guess the next: `shape2d` from
`body2d`, `mouse_down` from `key_down`.

- A module is the noun for what it owns, singular unless it is a keyed store.
  The five plurals are `assets`, `settings`, `strings`, `events` and `types`
  [D2].
- The dimension is a lowercase `2d` or `3d` at the end, on both of a pair:
  `body2d` and `body3d`. A name with no twin stays plain (`sprite`) [D5, N5].
- A reader is named for what it answers, with no `get_`, and `is_` only for a
  yes or no. Its setter is `set_` and the reader's name, beside it [N7, N8].
- A flag or a mode is never in a function's name. Options go in a trailing
  `<Function>Options` record, and a fixed choice is a `FAMILY_VALUE` constant
  [N9].
- A component's schema says `kind` for a variant and `type` for a datatype,
  and a property never repeats its component's name [N6]. A method on a
  component's handle leaves the component out: `node.softbody2d.area()`
  [N26].
- A bare hook is the engine asking (`init`, `update`), and
  `on_<event>(payload, source)` is the engine telling [N22].
- A theme token is `<group>_<role>` and never a hue. A theme key is the widget
  property it styles. A role is `<component>[_<context>][_<emphasis>]`, and a
  state is a sub-table, never a suffix [N18, N19, N20].

## D. The name says what the call site cannot

At a call site a reader sees only the name and its arguments.

- A key or a parameter carries its unit, unless it is the module's own:
  `timeout_seconds`, `delay_ticks`, `max_rate_hz`, `long_press_slop_pixels`,
  `spread_degrees`. Radians and seconds are the default [N21].
- The name says the dimension (`body2d`), a yes-or-no answer (`is_sleeping`),
  and whether a thing happened or is asked for (`finished`, `quit_requested`).
- `global_` is world space, `effective_` is resolved through a node's
  ancestors, and `just_` is this frame.

## What a name costs

Where a name appears decides what changing it breaks.

| Scope | Where the name appears | Cost of changing it |
| --- | --- | --- |
| `script-api` | Module and function names scripts call | Breaks existing projects, at `balaur export` |
| `scene-file` | Component keys, property names, enum options | Breaks existing `.toml` scenes and the inspector generated from the same schemas |
| `theme` | Colour and size tokens, theme keys, state tables and role names | Breaks a person's own theme file: an unknown key is dropped without a word |
| `settings-path` | Keys under `project.toml`, `editor.toml` and the settings registry | A stale key is read as unset, so the setting falls back |
| `cli` | Commands, flags and environment variables | Breaks scripts and CI jobs that call `balaur` |
| `on-disk` | Files and folders the engine writes | Leaves old files behind; there is no migration before 1.0 |
| `editor-ui` | Words the editor shows | Free to change, but a word that differs from the API is two words for one thing |
| `rust-internal` | Rust items inside `crates/` | Compiler-caught; `CONTRIBUTING.md` holds their rules |

A bad pattern seen twice becomes a lint: `scripts/api_lints.py` holds the
script API, `scripts/house_lints.py` the Rust side, `scripts/string_lints.py`
the words.

## Conventions by area

What the principles give in each area, in one place. Every name itself is in
`docs/generated/`.

- **Input**: A key's string is its W3C `KeyboardEvent.code` (`KEY_A` is
  `KeyA`). A pad button is named by its position, so `GAMEPAD_BUTTON_SOUTH` is
  the same button on every pad. `down` is held and `just_pressed` this frame.
  A stick reads -1 to 1 with up as +1, a trigger 0 to 1. A binding string is
  `mouse:left` or `gamepad:south`.
- **UI**: A widget kind and the `ui::*` function that draws it share one word.
  `active` means held, as in CSS, and `checked` covers on, selected and
  current.
- **Scenes and rendering**: An angle a scene stores in degrees ends in
  `_degrees`. `tint` passes down to children and `color` is one drawable's
  own. A light's switch is `shadow_enabled` and a caster's `cast_shadow`.
  Layers are numbered 1 to 32.
- **Physics**: One unit is one metre. `apply_*` lasts one step and
  `add_constant_*` until cleared; an `_at_point` call takes a world point. A
  size is the full extent, and a capsule's height runs tip to tip.
- **Editor**: A button is a verb, and a command that opens a dialog ends in
  `…`.
- **Command line**: A command is a lowercase verb (`run`, `export`) or its
  protocol (`lsp`). A long flag is `--kebab-case`, `--no-<x>` turns one off,
  and an environment variable is `BALAUR_` and its settings path in capitals.
- **Files**: The editor keeps `<data>/balaur-editor/` and a game
  `<data>/balaur/<name>/`, so no game's name reaches the editor's files.
- **Hooks**: `init`, `update`, `fixed_update`, `on_free`, `exports` and
  `on_hot_reload`. There is no `late_update`, `draw` or `input` hook: draw
  from `update` with `render::draw_*`, and read input through bindings.

## Decisions

Names the principles leave open, or bend, and why. Recorded so none becomes the
precedent for the next.

| Name | Why |
| --- | --- |
| `asset` for game content, `resource` for the typemap | Godot's `Resource` would take the Rust typemap's word (Bevy's, for the Rust reader), so content takes Unity's word, and five of six surveyed engines' (D1) |
| `rotation_euler` | The Rust field is a quaternion, so a bare `rotation` turns ambiguous the day a quaternion reader lands |
| A clip, `[clips.<name>]` | Unity's word, because `animation` is already the component |
| `volume_linear` | Godot reads a bare `volume` as decibels |
| `stems` for the files mixed into one sound | `layers` is a scope's word for masks |
| `pixels_per_unit` | Unity's word for a 2D camera's scale |
| `node.get_node`, `node.get_component` | Keep `get_` (N7): without it they read `node.node(path)` and `node.component(name)` beside `node.component_names()` |
| `render.camera_pose` | Reads what the backend applied for a `camera3d`, not what was asked; its doc line says so (N8) |
| `widget.x`, `widget.y` | Offsets from one of five anchor corners, not a position vector |
| `render` as one module | Revisited at 58 functions, when the window's eight moved to `window`. Revisit past about 70 |
| The editor's node types (`RigidBody3D`, `MeshInstance2D`) | Godot 4's class names, shown as they are to Godot users; one table, `model::display_types` |
| `scale`, four times | `node.transform.scale`, `ui.scale()`, a viewport's `scale_factor` and a 2D camera's `zoom` are four scopes, not one (N1) |
| `node.add_child` and `scene.instantiate` | Not synonyms: one empty node against a whole scene file |
| `platform.purchase`, not `apple.purchase` or `google.purchase` | Two stores sell the same things, so the verbs are the portable module's (B: one way to buy). A transaction's proof is `token` on every store, never StoreKit's `jws` |
| `[android] capabilities` spelled as `[apple]`'s | Kebab-case, and the same word where the thing is the same: `in-app-purchase` on both; `play-games` beside `game-center` |
| `platform.register_for_push`, not `apple.register_for_push` | APNs and FCM hand out the same thing, a token a server sends to, so the verb is portable beside `platform.notify`. Its answer spells `token`, `provider` and `platform` as Gamend's `push_tokens` does, and a replaced token is `push_token_changed`, past tense as `visibility_changed` |
| `[android] notification_icon`, not `[application] icon_notification` | The `icon_*` keys are forms of the app icon every platform shows; a status-bar silhouette is Android's alone, and Firebase's `default_notification_icon` names it the same way round |
| `[apple] macos_provisioning_profile` beside `ios_provisioning_profile` | One key a platform, as `macos_identity` and `ios_identity` are, and one `--provisioning-profile` flag for both, since an export has one target |
| `platform.notify`, not `device.notify` | Notifications sit beside the store verbs a game reaches for on every platform, as Apple's did; `device` holds what a phone's hardware and sheets lend |
| `steam_*` keeps Valve's names | Generated from Valve's `steam_api.json`: `SteamUser()->GetSteamID()` is `steam_user::get_steam_id()`, `get_`, plural modules and abbreviations kept, so Valve's documentation reads 1:1. A leading `B` on a bool reader is dropped |
| `balaur_core`, `balaur_import`, `balaur_cli` words | N17 met in part: each keeps a `vocabulary.rs` for the words it compares, core keeps the rest in the module that owns them, and the importer spells the scene keys it writes |

## Glossary

The words B fixes, one meaning each.

`resource` = typemap entry. `asset` = game content from a file. `system` = a
closure in a stage. `load` = produce a live object from a path. `source` = raw
file text. `spawn` = one empty node. `instantiate` = a scene file. `duplicate` =
a private copy of an asset. `export pack` = the `.bpak` an export writes,
never a bare `pack`. `kind` = a tagged-union discriminant. `type` = a schema
property's datatype. `prefab` = a scene used as a template. `binding` = an
event row on a node. `recording` = a play run on disk, replayed.

`token` = a named colour or size in a theme. `source` in a theme = one of the
seven colours or four sizes every other token is derived from. `role` = a named
look a widget takes. `workspace` = Scene, Script, Animation, Physics or UI.
`panel` = one tab's content. `dock` = the area holding panels. `dialog` = a
sheet over the editor. `popup` = a menu or picker anchored to a control.
`window` = the OS window only.
