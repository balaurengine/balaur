# Plugins: one trait, one order, one switch

Status: **phases 1-3 shipped.** Phase 4 is not started.

A module is declared in four places today — a cargo feature, an optional
dependency, two `pub use` lines, and a `load` call under `#[cfg]`. Extensions
are discovered and ordered; modules are neither. This is the plan to make a
plugin one declaration, ordered by one rule, and switchable from the project
rather than from the build.

## Where it stands

Two traits, two load paths:

| | `balaur_core::Plugin` | `balaur_plugin::Plugin` |
| --- | --- | --- |
| Shape | `name()`, `build(&mut App)` | `manifest()`, `declare(&mut Registry)` |
| Implemented by | input, physics, animation, render, ui | audio, http, websocket, gamend, every extension |
| Ordered by | the sequence written in `standard_app` | `load_order` — requirements, then name |
| Recorded | `App::plugins`, which nothing reads | nowhere |

`load_extensions_in` already does the whole job for dylibs: scan, read each
manifest, topologically sort, load. The built-in modules never reach it, so
`requires` cannot name a module and no code can ask what is loaded.

Three concrete gaps:

- `App::plugins` is written at `app.rs:288` and read nowhere, and
  `balaur_plugin::load` does not write it at all.
- `Registry` has no `register_asset_type`, no presets, and takes `fn` pointers
  where `balaur_input` passes closures, so the core five cannot move to it
  unchanged.
- `ARCHITECTURE.md` claims nothing in tree uses `Registry::app()`.
  `balaur_http` and `balaur_websocket` both do, to read `ProjectFiles`.

## What discovery can and cannot be

Two asks hide under "self-discoverable", and only one of them is about
linking.

**Discovery** — the engine finding a statically linked plugin with no list —
is `inventory` or `linkme`: a link-section the crate submits into. It is not
worth it here. It removes the `load` line and leaves the cargo feature and the
optional dependency, which is most of the work. Link order is not
deterministic across linkers, so the result has to be sorted anyway — by
`load_order`, which exists. And the failure mode is silence: an rlib nothing
link-references can be pruned and its submission simply disappears, where
`balaur`'s `pub use` is not a link-time reference. Across
`wasm32-unknown-emscripten`, iOS and Android, life-before-main sections are
the least reliable part of the toolchain. `manifest.rs` already states the house rule for exactly this shape
of risk: `env!` over `option_env!`, so a missing stamp fails the build instead
of passing quietly.

**Selection** — saying which plugins a game wants — is a data question, and it
is the half worth building. The compiled-in set is a *ceiling*: what this
binary can do. The project picks from it. Asking for something the build does
not have is an error that names the cargo feature to rebuild with, not a
silent omission.

## The four phases

**1. Record what loaded.** `crates/balaur_core/src/plugins.rs` holds a
`PluginRegistry` resource of `PluginInfo { name, version, requires }`, with
`loaded`, `names` and `is_loaded` beside it — the shape `components.rs`
already uses. Both load paths record, after the plugin's own registration
succeeds, so a plugin that failed is not one another can claim to require.
The write-only `App::plugins` field becomes an `App::plugins()` reader over
the resource, which puts the answer where a system can reach it too.

`balaur_plugin::load` now checks `requires` against that registry, so the
field is enforced on the module path and not only inside `load_order`, and
`load_order` takes the names already loaded — which is what lets an extension
require a module the binary linked in.

**2. One table.** A `modules!` macro in `balaur` generates the crate alias,
the plugin re-export and the constructor for each optional module, so a module
is one line in one file instead of four lines in two. It expands to a
`load_modules` that hands the set to `balaur_plugin::load_all`, which orders
the whole set before any of it registers — a missing requirement is refused
before half the set has already changed the app.

Two things the implementation decided differently. `webtransport` is a
`Transport` a project names at run time, not a plugin, so it stays a plain
re-export and the table holds only what registers. And `apple` turned out to
be the first real `requires` in tree: it calls `balaur_platform::set_backend`
from `declare`, which had been held by a comment and by hand-ordering.
`PlatformPlugin` moved ahead of the optional block and `apple` names it, so
the constraint is now enforced rather than remembered.

**3. One order.** Every plugin goes into one `Vec` and through `load_all`
once, so `standard_app` is a list rather than a sequence and `requires` is
believed wherever it is written.

The plan said "one trait" and the code stopped short of it, deliberately. The
engine's own five build against the whole `App` — asset types, presets,
components, closure-taking replay sources — and none of that is on `Registry`.
Rewriting five crates to reach it all back through `app()` would have bought
the word "one" and nothing else. Instead `balaur_plugin::Builtin` wraps a
`balaur_core::Plugin` in a manifest, which is what the ordered set actually
needs, and leaves those five untouched.

So two traits remain, but not two paths: `balaur_core::Plugin` is now the
shape an in-tree plugin is *written* in, adapted at the boundary, rather than
a second way to load one. Collapsing it needs `Registry` to grow asset types,
presets, components and a closure-taking replay source — the same list the C
boundary needs, which is the argument for doing both at once rather than
neither.

**4. One switch.** `[plugins]` in `project.toml`:

```toml
[plugins]
http = false
```

Loaded by default when compiled in; `false` turns one off; a name the build
does not have is an error naming its feature; a name that is neither a module
nor an extension is an error. Turning off something another plugin requires
already produces the right message from `load_order`. `ProjectManifest` does
not deny unknown fields, so the table is backwards-compatible.

An export inherits the switch, and should: a project that disables http and
ships a script calling `http.get` fails `build_pack`, because `build_pack`
boots through `standard_app`.

## The one risky change, and why it is safe

Phase 3 replaced a written sequence with a sorted one. Before:

    input, physics, animation, render, platform, [apple, audio, gamend, http, websocket], ui

After: `animation, audio, gamend, http, input, physics, platform, render, ui,
websocket`, with `apple` pulled up behind `platform` by its requirement. Registration order is system order within a stage, and system
order is the simulation, so this needs an argument rather than a hope.

No built-in plugin requires another. Presets and components are registered
into maps keyed by name and validated when applied, not when registered, so a
preset naming another plugin's component does not constrain load order. The
only stage several plugins share is `Stage::First`: input's gamepad poll and
action tick, and the http, websocket and gamend pumps. Sorting moves the pumps
across the input systems. They share no state — the pumps write network
arrivals, input writes input, and scripts read both in `Stage::Update` — so
the tick digest is unchanged. `drive_ui_focus` is added by the facade after
every plugin and stays last in `First`, which is what makes it read the
actions of the frame it runs in.

Two further checks the sort turned out to need. `balaur_anim` depends on no
other plugin crate, and nothing in `balaur_ui`'s build reads a resource
another plugin inserts, so moving `animation` to the front and `ui` ahead of
`websocket` changes nothing. And `scripts/gen_docs.py` sorts components,
asset types and `api.json` keys, so registration order does not reach the
generated reference.

## Open

- Whether `[plugins]` values should be tables carrying per-plugin config, or
  stay booleans while each plugin keeps reading its own file through
  `ProjectFiles`. The second is what audio, http and websocket do now.
- Whether a script should be able to ask what is loaded, or only tooling.
