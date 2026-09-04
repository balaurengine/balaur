> **Status:** phases 1-8 shipped between 2026-09-02 and 2026-09-04 — WESL
> shaders linked at build and at run time, the `material` asset in 2D and 3D,
> hot reload with sourcemapped errors, `features` variants, channel views,
> the caret value preview and `render::shader_probe`, headless `eval` tests
> and validation, and `shaders::register_shader_module` for a plugin's own
> module. ARCHITECTURE.md's shader sections and the manual's Shaders page are
> the record; all of it is verified on a GPU through `balaur run --offscreen`.
> Phase 9 is what is left.

# Plan: shaders — what is left

## Post-process materials

`camera.post` runs the engine's own passes — bloom, SSAO, SSR, depth of
field. A user shader on that chain is the same `material` asset over a
full-screen pass: the input is the rendered colour (and depth, where a pass
wants it), the output replaces it, and `features` picks a variant as anywhere
else. What has to be decided is the order — where a user pass sits among the
built-in ones — and whether a material declares that or the camera does.

## Shader packages

The plugin half is built: `shaders::register_shader_module` mounts a plugin's
module beside the engine's own, so a project's shader imports it like any
other. What is left is publishing Balaur's own helpers as a crates.io
package, which is a release action rather than engine work.

## Open questions

1. **Where the param schema comes from.** `render::material_params` parses
   the shader's `Params` struct and derives the inspector's rows, which
   cannot drift from the shader. Where a range or a description would live is
   still open: the material would have to annotate what the struct declares.
2. **When linking happens in an export.** Linking at load costs milliseconds
   per material on a cold start; linking at export time puts WGSL in the pack
   and drops the compiler from the shipped binary. Export-time is the answer,
   once `balaur export` learns to walk materials.
3. **Whether kiss3d should expose its linker.** `compile_wesl` and
   `package::common` are `pub(crate)`. If they were public, Balaur's shaders
   would import the engine's helpers instead of keeping a second copy. Worth
   an upstream ask before writing the second copy.
4. **How far the interpreter goes.** Shader tests use the pure functions and
   work. A CPU probe over textures, samplers and interpolated inputs may
   simply not be there yet — the caret preview reads the GPU instead, so
   nothing is blocked on the answer.
5. **A shader graph.** Not planned, but the module system is what would make
   one honest: a node is a function in a module, a graph is imports and
   calls, and the output is readable WESL a user can take over. Every graph
   built on string concatenation regrets it. If it is ever built, it is built
   on this.
6. **Compute shaders.** Out of scope. A compute pass that wrote back into
   simulation state would break the observer rule; one that only feeds
   rendering (particles on the GPU) is a later plan, and needs the same
   headless answer particles already have.
