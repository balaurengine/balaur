# Plan: custom file formats

A project's own file formats, as content the engine loads, reloads and ships.
Nothing here is built; the questions below come first.

## What there is

- A Rust plugin registers an asset type, `App::register_asset_type`, whose
  definitions are TOML with `type = "<name>"`. Saving one forgets its parse
  and moves the asset generation.
- Any other file a game reads itself, `fs::read("data/one.level")`. A save
  names it in `files_changed`, and the script that listens reads it again.
  The asset generation does not move for it.
- Such a file ships when an export `include` glob names it.

## Steps

1. **A reader for a file extension.** A plugin, in Rust or in script,
   registers an extension with a function from the file's bytes to a value.
   Loading the path caches the value, and a save forgets it and moves the
   generation, as a texture does. Done when a test saves a `.level` file
   under a running scene and the node reading it sees the new value.
2. **Shipping and listing.** A registered extension ships in an export pack
   with no `include` glob. The Assets dock gives its files a card and an
   icon, and a property naming one offers them in its picker.

## Open questions

1. **Script readers.** Whether a reader written in script can run inside an
   asset load, which happens mid-instantiate and mid-frame.
2. **What a reader hands back.** `assets.load` hands a script a table. A
   format read into something else, a mesh or a grid, may need a Rust reader.
3. **Import settings.** Whether a custom kind takes a sidecar,
   `one.level.import.toml`, and a `[import.<kind>]` default in `project.toml`.
