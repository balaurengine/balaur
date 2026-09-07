> **Status:** phases 1-8 shipped between 2026-09-02 and 2026-09-04 — WESL
> shaders linked at build and at run time, the `material` asset in 2D and 3D,
> hot reload with sourcemapped errors, `features` variants, channel views,
> the caret value preview and `render::shader_probe`, headless `eval` tests
> and validation, and `shaders::register_shader_module` for a plugin's own
> module. ARCHITECTURE.md's shader sections and the manual's Shaders page are
> the record; all of it is verified on a GPU through `balaur run --offscreen`.
> Phase 9 is what is left, and its design was settled on 2026-09-07: the
> camera lists its passes in order. What blocks the build is named below and
> is not in this repository.

# Plan: shaders — what is left

## Post-process materials

`camera.post` runs the engine's own passes — bloom, SSAO, SSR, depth of
field. A user shader on that chain is the same `material` asset over a
full-screen pass: the input is the rendered colour (and depth, where a pass
wants it), the output replaces it, and `features` picks a variant as anywhere
else.

**The camera lists its passes, in order. Decided 2026-09-07.** `post` stops
being a flag set and becomes an ordered list mixing the built-in names with
material ids:

```toml
[nodes.camera]
post = ["ssao", "colour_grade", "bloom"]
```

What runs is what the list reads, top to bottom. The alternative was a
`stage = "after_tonemap"` on the material, which makes one material reusable
across cameras but needs a slot vocabulary the engine has to keep meaningful
as passes change, and leaves two materials in one slot with no order between
them. A third — a `post_materials` key that always runs last — was rejected
for the limit it makes permanent: no fog before SSAO, no grade before bloom.

The cost of the decision is that the four built-ins gain a real order Balaur
hands the renderer, rather than one the renderer fixes.

**What blocks it is in the fork, not here.** `camera.post` reaches kiss3d as
four independent booleans — `set_bloom_enabled`, `set_ssao_enabled`,
`set_ssr_enabled`, `set_dof_enabled` (`kiss3d_backend.rs`, `apply_post`) —
and their order lives inside the HDR pipeline. `Window` takes no chain and no
user pass: `PostProcessingEffect` exists in the fork but is not wired to
`Window`, and its own documentation says one effect at a time. So the fork
needs an ordered chain that runs built-in passes and user WGSL passes over
the HDR film before anything here can honour the list above. Landing the
schema first would promise an order the renderer ignores, which is why it
has not been.

The Balaur half, once the fork can take a chain: `post` parses to an ordered
`Vec` of built-in-or-material, `PostConfig` carries it, and a material in the
list builds the pipeline `shader_material.rs` already builds for a screen
reader, over the whole film rather than around one object.

## Shader packages

The plugin half is built: `shaders::register_shader_module` mounts a plugin's
module beside the engine's own, so a project's shader imports it like any
other. What is left is publishing Balaur's own helpers as a crates.io
package, which is a release action rather than engine work. Left out of
phase 9 on 2026-09-07: it means owning a crate name and a version cadence
before anyone has asked for the helpers, and a project that wants its own
module already has `shaders::register_shader_module`.

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
5. **A shader graph.** Planned for 1.0 now that `docs/PLAN-authoring-without-code.md`
   brings a node canvas: a graph emits WESL the way that one emits Rune. The
   module system is what makes
   one honest: a node is a function in a module, a graph is imports and
   calls, and the output is readable WESL a user can take over. Every graph
   built on string concatenation regrets it. If it is ever built, it is built
   on this.
6. **Compute shaders.** Out of scope. A compute pass that wrote back into
   simulation state would break the observer rule; one that only feeds
   rendering (particles on the GPU) is a later plan, and needs the same
   headless answer particles already have.
