# A C API for addons

Tier 1 is built: script modules of functions and constants over a C ABI,
`crates/balaur_plugin/src/capi.rs` and its header
`crates/balaur_plugin/include/balaur_extension.h`. This plan holds what an
addon would need next.

## What Tier 1 does not do

1. **Tier 2: components, systems and scene keys.** An extension registers a
   component type, a per-frame system and a scene key across the ABI. It waits
   for an addon that asks, since the component and query ABI is costly to
   change once published. Decide first whether component data crosses as a
   schema or as `Value`. Done: an example beside `examples/extension_c_counter`
   registers a component and a system, and a scene sets the component's key.
2. **Calling back into script.** A script hands an extension a function, and
   the extension calls it during its own call. Done: a test in
   `crates/balaur_plugin/tests/suite/` where C calls a script function it was
   given and returns the answer.

## Open questions

- Are addons for the editor (tools, importers, panels) or for games (native
  gameplay code)? The answer decides what Tier 2 exposes first. The editor is
  a Balaur project, so its addons may be better served by scripts.
- A plugin can read its own file through `ProjectFiles` or its `[plugins]`
  table through `Registry::config`. No plugin in tree reads the second. Settle
  which one a plugin prefers before one grows both.
