# Plan: per-platform project settings

A game that ships to a phone and to a desktop wants two answers to the same
question: fullscreen here, landscape there; the solver's iterations here, two
of them there; this texture here, a smaller one there. `project.toml` has one
answer per key.

Godot's answer is a feature tag suffixed onto the key, resolved when the
setting is read rather than when the game is exported, so the editor can play
a project as any target. That is the right shape, and the work below is mostly
what has to be true before it fits: today a table in `project.toml` reaches
code by four different routes, and an override written once would have to be
implemented in each.

## Where it stands

| Route | Who | Tables |
| --- | --- | --- |
| A typed field on `ProjectManifest`, parsed at load | `crates/balaur_core/src/project.rs`, read at `App::load_project` | `[application]`, `[window]`, `[ui]` |
| The raw manifest text, re-parsed by a plugin on the first tick | `crates/balaur_physics/src/tuning.rs` through `project::manifest_source` | `[physics]` |
| A typed loader re-reading the file from disk, at export | `crates/balaur_export/src/{config,android,apple}.rs` | `[export]`, `[android]`, `[apple]` |
| `settings::define_group`, a description mirrored against one of the above by hand | `crates/balaur_core/src/settings.rs` | `application/*`, `physics/*`, `save/*`, `locale/*` |

The fourth is the one with the properties everything else wants: a path that
is also the storage (`physics/solver_iterations` is `[physics]
solver_iterations`), a spec carrying the default, the range and the help, a
screen that groups and searches every one of them, and a script API a game
declares its own settings through. It just does not own the values. Nothing
loads `project.toml` into `SettingsValues` in a running game, so the registry
is a description of settings the engine reads somewhere else.

Three smaller facts that shape the work:

- `settings::to_toml` says in its own doc comment that it keeps a manifest's
  comments. It does not: it round-trips through `toml::Table`, which drops
  them. `toml_edit` is already a `balaur_core` dependency and is used exactly
  this way in `asset_index.rs`.
- The three export loaders call `std::fs::read_to_string` rather than the
  `files` backend, so exporting from the browser editor cannot work as
  written.
- `PlatformFacts` carries `os`, `web` and `mobile` but no architecture, and it
  is the recorded set: what a replay answers from is what a resolver should
  read, so a recording made on a phone replays the phone's settings.

## What changes

1. **One reader.** `App::load_project` folds the manifest text into
   `SettingsValues` before anything reads a setting. `[physics]` stops
   re-parsing `manifest_source`; `[window]` and `[ui]` stop being typed fields
   on `ProjectManifest` and become declared groups the renderer and the theme
   read through `settings::get`. The defaults move from a `Default` impl into
   the schema, which is where the editor already looks for them.
2. **Tags.** One vocabulary, computed once per run and held as a resource:
   the group (`desktop`, `mobile`), the os (`windows`, `macos`, `linux`,
   `android`, `ios`, `web`), the architecture (`x86_64`, `arm64`, `wasm32`),
   the build (`debug`, `release`, `editor`), and whatever custom names a
   target adds. Derived from the same facts a recording restores, and cheaply
   enough that reading one does not build a device id.
3. **`[override.<tag>]`.** An override is the setting's own path under
   `override/<tag>/`, which means the storage rule needs no exception:

   ```toml
   [override.android.window]
   fullscreen = true
   orientation = "landscape"

   [override.mobile.physics]
   solver_iterations = 2.0
   ```

   `settings::get` tries each active tag, most specific first, and falls back
   to the base value. Precedence is a declared order — group, os, arch, build,
   custom, later winning — rather than Godot's any-tag-matches, which leaves
   two overrides on one key racing on file order.
4. **A base read, for the editor.** `settings::base` answers what the file
   says rather than what this machine resolves, so the settings screen edits a
   value instead of showing the override the editor's own platform picked.
   Godot's `disable_feature_overrides`, as a call rather than a mode.
5. **Every table declared.** `[window]`, `[ui]`, `[export]`, `[android]`,
   `[apple]` and `[import.<kind>]` each get a schema block with help, ranges
   and `applies`. That is the whole of what makes them searchable and grouped:
   the settings screen is driven by the registry, so a declared table appears
   in it without a line of editor code.
6. **The export sheet becomes a view.** `editor/scripts/exporter.rn` keeps the
   target rows and the button, and the fields it currently explains in a
   sentence become deep links into the settings screen, which `open_at` and
   `open_search` already serve. The three loaders in `balaur_export` read
   through the registry and the `files` backend, not `std::fs`.
7. **An override button.** A row in the settings screen offers "override
   for…", which writes `override/<tag>/<path>` and shows the overrides a key
   carries beneath it. `settings::set` needs no change; the path is the
   feature.
8. **What is baked, not resolved.** Three things cannot wait for a read:
   the bytes in the pack, the native manifests (`AndroidManifest.xml`,
   `Info.plist`, `index.html`), and the custom tags themselves. Export
   resolves those against the target's tags and writes them into the artefact,
   the way Godot bakes `_custom_features`. Everything else stays a read.
9. **Asset variants.** A variant is a file beside the asset, named by tag:
   `sprites/hero.png` and `sprites/hero.android.png`. The pack is a map keyed
   by project-relative path, so the exporter substitutes the bytes under the
   canonical path and nothing downstream — scenes, scripts, the runtime —
   learns a new concept. A variant that no tag selects never enters the pack.
10. **`[window] mode` and `orientation`.** The window keys are desktop-shaped:
    a phone has no width. `mode` (`windowed`, `borderless`, `fullscreen`) and
    `orientation` (`any`, `portrait`, `landscape`) join them, and both reach
    the two native manifests through step 8 rather than through a raw plist
    key.

## What not to do

- **No resolution at export for settings.** Baking is a second code path for
  `balaur run` and the editor's Play button, and it puts a target the editor
  cannot preview inside the file it is editing.
- **No per-platform tick rate.** `TICK_HZ` is one declaration on purpose, the
  replay header does not record it, and lockstep between a phone and a desktop
  would desync by construction. Per-platform frame *pacing* is a different
  key and is fine. If the simulation step ever moves, the header records it
  first and a replay refuses a mismatch.
- **No unknown key inside a declared table.** `[window] fullscren` is an
  error. An undeclared top-level table is not: `[mygame] local_server_url` is
  the game's own space, readable as `settings.get("mygame/local_server_url")`
  and promotable to a real row with `settings.define`.
- **No second file.** Godot keeps export presets in `export_presets.cfg`,
  outside project settings and outside its search. The reason is credentials,
  and `[export]` already answers that by reading secrets from the environment.
- **No feature-tag syntax on keys.** `fullscreen.android` is not TOML without
  quoting, and a quoted key is not a path the storage rule already nests.

## Worth checking when this is picked up

Whether `[plugins]` belongs under an override at all. Turning a module off on
one platform is a run-time selection out of what the template already linked,
so it saves nothing but a little startup — the size win is the separate web
module split. It is still the honest place to say "no `http` on iOS", but the
row should not promise a smaller binary.
