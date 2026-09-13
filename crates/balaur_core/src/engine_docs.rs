//! The prose beside every engine op: one `module_doc` and one `describe` per
//! module, which is what the generated reference and the editor's Docs dock
//! render. Kept apart from the table that names the ops.

use crate::engine::Engine;

pub(crate) fn document_engine(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The running app itself: the clock a frame reads, the command line it \
         was started with, the directory it may write to, and the way out.",
    );
    m.describe(&[
        ("time", &[], "()", "Seconds of engine time since the app started, accumulated as a float."),
        ("timings", &[], "()", "What the last frame cost, in seconds: `{ frame, fixed_steps, stages, spans }`. Presentation only: branching a `fixed_update` on wall time desyncs, and nothing records it."),
        ("profile_scripts", &[], "(on)", "Start or stop counting what each script costs. Turning it on clears the tally."),
        ("script_costs", &[], "()", "What each script has cost since `profile_scripts(true)`, dearest first: a list of `{ path, calls, instructions }`. Instructions, not seconds, so the number is the same on every machine."),
        ("delta", &[], "()", "Seconds the frame in progress covers, the same number a system is handed."),
        ("tick", &[], "()", "Which frame this is, counted whole: what simulation code branches on instead of `time`."),
        ("quit", &[], "(code: int?)", "Ask the app to shut down; the frame in flight still finishes, and the process exits with `code`, 0 when left out."),
        ("args", &[], "()", "The command-line arguments the app was started with, empty when it was given none."),
        ("reload_script", &[], "(key: string)", "Recompile one script by its project-relative key, for a tool editing files outside the watched root."),
        ("user_data_dir", &[], "()", "A writable per-user directory for saves and settings, created on first call and named after the project."),
        ("open_url", &[], "(url: string)", "Open an http, https or mailto URL in whatever the player browses with: an opener on a desktop, a new tab on the web. Not on iOS or Android yet, where it reports that it has no opener. An effect on the world outside the game: never recorded, and it does nothing while a recording plays."),
        ("reveal", &[], "(path: string)", "Show a file or directory in the system file manager, selected where the platform can. Desktops only: neither a browser tab nor a phone has a file manager to ask. Never recorded, like `open_url`."),
        ("plugins", &[], "()", "Every plugin this build loaded, named, in load order."),
        ("has_plugin", &[], "(name: string)", "Whether one plugin loaded, so a game shipped without `http` can say so rather than call into a module that is not there."),
        ("plugin_version", &[], "(name: string)", "The version of one loaded plugin, or nil when it did not load."),
        ("platform", &[], "()", "Where this runs: `{ os, web, mobile, touchscreen, editor }`. Recorded in a session's header, so a replay on another machine answers as the original did."),
        ("device_id", &[], "()", "One id per install, made on first use and kept in the user directory: what a device login sends. Recorded with the session."),
        ("unix_time", &[], "()", "The wall clock at the top of this tick, in seconds since 1970. Read once per frame and recorded, so a replay sees the time the recording saw."),
        ("focused", &[], "()", "Whether the window is in front of the player this tick; every script's `on_focus_changed(bool)` is called when it changes. True with no window."),
        ("dark_mode", &[], "()", "Whether the system is in dark mode this tick; every script's `on_dark_mode(bool)` is called when it changes. False where nothing says."),
    ]);
}

pub(crate) fn document_hash(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc("Content hashes, for a download a game verifies before it trusts it.");
    m.describe(&[
        ("sha256", &[], "(path: string)", "The SHA-256 of a file as lowercase hex, read through the project's file roots; an absolute path is read as given."),
        ("sha256_text", &[], "(text: string)", "The SHA-256 of a string as lowercase hex."),
    ]);
}

pub(crate) fn document_encoding(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc("Bytes as text and back, for what a server hands over in base64.");
    m.describe(&[
        (
            "base64",
            &[],
            "(data: bytes | string)",
            "Bytes, or a string's UTF-8, as standard base64 with padding.",
        ),
        (
            "from_base64",
            &[],
            "(text: string)",
            "The bytes a base64 string encodes; an error for text that is not base64.",
        ),
    ]);
}

pub(crate) fn document_scene(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The node tree: its root, lookup by path, instancing. \
         Also the component and preset vocabulary an editor builds its \
         palette from.",
    );
    m.describe(&[
        ("root", &[], "()", "The tree's root node."),
        ("get_node", &[], "(path: string)", "The node at an `A/B/C` path from the root, where `..` climbs to the parent; nil when nothing matches."),
        ("node_by_id", &[], "(id: string, under: node?)", "The node carrying a stable id, which survives the rename and the reparent a path does not; nil when nothing carries it. `under` bounds the search to one subtree, for a tool holding more than one tree."),
        ("with_component", &[], "(component: string)", "Every node carrying the named component, in tree order. What a script asks instead of walking the tree itself."),
        ("tagged", &[], "(tag: string)", "Every node filed under a tag, in tree order; what a scene's `tags` key and `node.add_tag` feed."),
        ("instantiate", &[], "(source: string, parent: node?, opts: any?)", "Build a scene document (TOML text, not a path) under a parent; `{ scripts: false }` leaves scripts unattached."),
        ("source", &[], "(path: string)", "A scene file's raw TOML text, project-relative and found inside the pack in a packed run; nil when missing."),
        ("component_types", &[], "()", "The names of every registered component type, not the components on any node."),
        ("component_tags", &[], "(name: string)", "The facets a component type is filed under, for filtering a palette; nil for a name nothing registered."),
        ("component_expects", &[], "(name: string)", "The components a component type needs something from, for ordering or grouping its sections; nil for a name nothing registered."),
        ("component_schema", &[], "(name: string)", "A component type's property schema as a table; nil for a name nothing registered."),
        ("component_properties", &[], "(name: string, params: any)", "What a component's `apply` would receive for `params`: the schema's defaults with a partial table merged over them. This is how a tool compares two spellings of the same component."),
        ("presets", &[], "()", "The names of every registered preset."),
        ("preset_info", &[], "(name: string)", "A preset's description, tags and the components it adds; nil for a name nothing registered."),
        ("apply_preset", &[], "(node: node, name: string)", "Add every component a preset names to the node; a part that fails leaves the parts before it in place."),
        ("unmet_expectations", &[], "(node: node)", "Components on the node whose expectations nothing satisfies, as `{ component, expects }`; advisory only."),
        ("variable", &[], "(name: string)", "A scene variable's value, or nil for a name nothing declared. A scene declares them under `[variables]`."),
        ("set_variable", &[], "(name: string, value: any)", "Write a scene variable, coerced to the type it was declared with. Every node declaring `on_variable_changed` hears about it at the end of the tick; writing the value it already holds says nothing."),
        ("variables", &[], "()", "Every declared variable as `{ name, type, value, persist }`, in name order."),
        ("switch", &[], "(path: string, options: map?)", "Replace the scene with another one at the end of this tick, so a script asking inside `update` is not freeing the tree it runs in. `fade` is seconds the renderer crosses over; reset is a switch to the same file."),
        ("bindable_events", &[], "()", "Every event a `[[nodes.bindings.rows]]` row may answer, in the order an editor offers them."),
        ("binding_actions", &[], "()", "Every action a binding row may do, in the order an editor offers them."),
    ]);
}

pub(crate) fn document_skeleton(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Bones under a rig node: the rest pose a rig returns to, and the tree \
         order a skin numbers its joints in. A bone is any node carrying \
         `bone2d` or `bone3d`; there is no skeleton component.",
    );
    m.describe(&[
        ("apply_rest", &[], "(node: node)", "Move every bone under the node back to its rest transform."),
        ("overwrite_rest", &[], "(node: node)", "Record every bone's current transform under the node as its new rest pose."),
        ("bones", &[], "(node: node)", "The bones under the node in tree order, the order a skin numbers them in, the node itself first when it is one."),
    ]);
}

pub(crate) fn document_assets(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Asset definitions by reference: a project-relative file path, \
         `file#entry` for one entry inside it, `#id` for a block the scene \
         declares, or `id://<id>` for a path `assets/index.toml` names so the \
         reference survives a rename. A script gets the definition table, not \
         the parsed object the owning plugin builds from it.",
    );
    m.describe(&[
        ("load", &[], "(reference: string)", "The definition table behind a reference, from the cache; an error when the reference resolves to nothing."),
        ("duplicate", &[], "(reference: string)", "A private copy of a definition, read past the cache, so editing it disturbs no other holder of that reference."),
        ("exists", &[], "(reference: string)", "Whether a reference resolves to a definition that is really there; false rather than an error when it does not."),
        ("reload", &[], "(reference: string)", "Forget a reference so the next load re-reads its file, along with every entry cut from that same file."),
        ("invalidate", &[], "()", "Declare everything derived from project files stale (a shader a material links, say) so it is rebuilt from disk; for a file the watcher does not cover."),
        ("save", &[], "(reference: string, definition: any)", "Write a definition table to the project-relative file a reference names; an error unless it names a whole file."),
        ("directory", &[], "(type_name: string)", "The project-relative directory files of an asset type belong in; empty when the type is unknown or declared none."),
        ("rename", &[], "(from: string, to: string) -> [string]", "Move a file or directory and rewrite every reference to it in the project's `.toml` files, comments kept; answers the files rewritten. Paths as `fs.*` takes them, so an editor refactors the game it has open by absolute path. Script sources are not rewritten: a path in a `.rn` is the script's own value, and `id://` is the reference that survives a move."),
        ("id", &[], "(path: string) -> string?", "The id `assets/index.toml` gives a file, or nil when it has none."),
        ("assign_id", &[], "(path: string) -> string", "The id a file has, giving it one if it has none: a digest of the path and content, written to `assets/index.toml` and, for an asset document, as its top-level `id`. Reference it as `id://<id>` afterwards."),
        ("path", &[], "(reference: string) -> string", "The path an `id://` reference resolves to in the running project; a path comes back as itself, and an unknown id is an error naming the index."),
    ]);
}

pub(crate) fn document_strings(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Localization: one `strings/<locale>.toml` per language, keys to \
         strings. `[locale]` in `project.toml` sets the locale a run starts \
         in and the one a missing key falls back to. A key neither has comes \
         back as itself: visible in the game, which is how a missing string \
         gets noticed rather than showing as a blank label.",
    );
    m.describe(&[
        ("tr", &[], "(key: string, args: table?)", "The string for a key in the current locale. `{name}` in it is replaced by the argument called `name`, and an `n` argument also picks the plural form the locale's language calls for."),
        ("locale", &[], "()", "The locale in force."),
        ("set_locale", &[], "(locale: string)", "Switch locale; the next `tr` answers in it, which for a widget showing a key is the next frame."),
        ("locales", &[], "()", "Every locale the project ships a `strings/<locale>.toml` for, in name order."),
        ("system_locale", &[], "()", "The locale the operating system reports, like `en-US`, or nil when it says nothing; recorded with the session. A game picks its starting locale from it once and saves the choice."),
        ("set_root", &[], "(root: string)", "Read the catalogues from this directory instead of the project root, forgetting the ones already read; an empty string puts it back. For a host running a project other than its own: the editor, whose own root has no `strings/`, so without this every `text_key` in a played scene draws as its key."),
    ]);
}

pub(crate) fn document_save(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Save games: a table in, a table out, stored per user rather than in \
         the project. Nothing here is engine state: a save is whatever the \
         game puts in it, so what the engine decides is only where it lives, \
         that a half-written file cannot replace a good one, and what version \
         it was written at. `[save] version` in `project.toml` sets that \
         version and `[save] migrate` names the script whose \
         `migrate_save(version, data)` brings an older file forward, one \
         version per call.",
    );
    m.describe(&[
        ("write", &[], "(slot: string, data: any)", "Write a table to a named slot, stamped with the project's save version. Written beside the target and renamed over it, so a crash mid-save cannot destroy the last one."),
        ("read", &[], "(slot: string)", "The table in a slot, brought forward to this build's version; nil when the slot was never written. An error when the file was written by a newer build, or when it needs a migration the project declares no script for."),
        ("slots", &[], "()", "Every slot that has been written, in name order."),
        ("remove", &[], "(slot: string)", "Delete a slot. Not an error when it was not there."),
        ("version", &[], "()", "The save version this build writes, from `[save] version`."),
    ]);
}

pub(crate) fn document_log(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The three levels a script writes at, and the buffer behind them. \
         Scripted lines go through the engine's own `tracing` stream, so they \
         land beside engine ones.",
    );
    m.describe(&[
        ("info", &[], "(message: string)", "Write a line at info level, tagged as coming from a script."),
        ("warn", &[], "(message: string)", "Write a line at warning level, tagged as coming from a script."),
        ("error", &[], "(message: string)", "Write a line at error level, tagged as coming from a script."),
        ("recent", &[], "(n: int?)", "The last n buffered entries, 100 by default, each `{ time, level, tag, message, fields }`."),
        ("clear", &[], "()", "Empty the buffer, so a console reading it starts again from nothing."),
    ]);
}

pub(crate) fn document_rng(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "The engine's one deterministic PCG32 stream: the same seed draws the \
         same numbers on every platform, and a replay reproduces every draw a \
         recorded session made.",
    );
    m.describe(&[
        ("seed", &[], "(seed: int)", "Restart the deterministic engine stream at the given seed, so every draw after it repeats."),
        ("random", &[], "()", "A float from the deterministic engine stream, uniform in `[0, 1)`."),
        ("uuid", &[], "()", "A version-4 UUID drawn from the deterministic engine stream, so a replay makes the same ids; not for anything that must be unique across machines."),
        ("range", &[], "(low: float, high: float)", "A float from the deterministic engine stream, uniform in `[low, high)`: the two arguments."),
        ("int", &[], "(low: int, high: int)", "A whole number from the deterministic engine stream, uniform in `[low, high]`, both ends included."),
    ]);
}

pub(crate) fn document_fs(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "Files on disk, project-relative unless the path is absolute, so a \
         script cannot wander the filesystem by accident. This is the disk \
         itself: a packed build's contents are reached through `assets` and \
         `scene.source`.",
    );
    m.describe(&[
        ("read", &[], "(path: string)", "A whole file as text, project-relative unless absolute; nil when it cannot be read."),
        ("write", &[], "(path: string, text: string)", "Write text to a project-relative file, creating the directory it goes in."),
        ("exists", &[], "(path: string)", "Whether a project-relative path has anything at it, file or directory."),
        ("list", &[], "(path: string)", "A directory's entries as `{ name, is_dir }`, sorted, dotfiles skipped; empty for a directory that is not there."),
        ("remove", &[], "(path: string)", "Delete a project-relative file, or a directory and everything under it; false when there was nothing there."),
        ("mkdir", &[], "(path: string)", "Create a project-relative directory and every parent it needs."),
        ("rename", &[], "(from: string, to: string)", "Move a project-relative file or directory, creating the destination's parent first."),
        ("copy", &[], "(from: string, to: string)", "Copy a file byte for byte, creating the destination's parent first. What `read` and `write` cannot do for an image or a model."),
        ("mtime", &[], "(path: string)", "When a file last changed, in seconds since the Unix epoch; nil for one that is not there."),
    ]);
}

pub(crate) fn document_toml(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "TOML text to and from script tables: the format scene files, asset \
         definitions and component properties are all written in.",
    );
    m.describe(&[
        ("parse", &[], "(text: string)", "The table a TOML document describes; an error on text that does not parse."),
        ("encode", &[], "(value: any)", "A table written back out as TOML text; a node or callback in it is not data and is an error."),
        ("patch", &[], "(existing: string, table: any)", "The document's text with this table's keys written into it, keeping every comment, the key order and any table the value does not name. What a tool saving a hand-written file uses instead of `encode`."),
    ]);
}

pub(crate) fn document_json(m: &mut dyn balaur_script::Bindings<Engine>) {
    m.module_doc(
        "JSON text to and from script values, for talking to anything outside \
         the engine. Unlike TOML it has null, so nil survives a round trip.",
    );
    m.describe(&[
        ("parse", &[], "(text: string)", "The value a JSON document describes; an error on text that does not parse."),
        ("encode", &[], "(value: any)", "A value written back out as JSON text; NaN, infinity, a node or a callback has no JSON form and is an error."),
    ]);
}
