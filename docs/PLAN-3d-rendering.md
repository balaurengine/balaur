# Plan: the 3D look

What the 3D renderer in `crates/balaur_render` still lacks, over the kiss3d
fork. Lights, the physically based material contract, sky, glass, mirrors,
probes and the finishing passes are built.

## Steps

1. **A rendered still.** The fork's path tracer (`renderer/raytracer`) behind
   the editor's Export sheet (`editor/scripts/exporter.bs`), with a sample
   count and a denoise toggle, never a run mode. A material's
   `trace_surface` and `light3d`'s source radius are already parsed for it.
   Done when the sheet writes a traced image of the open scene.
2. **Limits a project sets.** `MAX_LIGHTS`, `MAX_PROBES` and `MAX_JOINTS` are
   constants in `shaders.rs`, fixing the arrays of the `Pod` structs in
   `frame_group.rs`, `skinned_2d.rs` and `skinned_3d.rs`. Read `[render] max_lights`, `max_probes` and
   `max_joints` at boot, write the frame uniform as bytes of that length,
   hand the same numbers to `shaders::link`, and refuse a limit the adapter's
   `max_uniform_buffer_binding_size` cannot hold, naming its figure. Done
   when a project naming 64 lights lights with 64.
3. **The film and the post chain cost less.** The film is the fork's
   `Rgba16Float` (`post_processing/hdr.rs`); R11G11B10F halves its bytes, and
   every pass on it is bandwidth. Tonemap, grading and the composite each
   load and store the film; fuse them into one pass. Done when `--timings`
   shows the frame's GPU time down on `examples/hello` and `examples/rig3d`.
4. **The editor viewport's tonemap.** Under `--timings` it reads 2.71 ms at
   1920x1080 where a game's reads 0.83 on the same Apple M1. Several passes
   share that timer scope: find which the editor adds, and time each apart.
5. **Screen-space reflections read a material's roughness.**
   `shaders/prepass.wesl` writes every surface as fully rough, so SSR reads a
   project material as diffuse. Done when the prepass writes the material's
   own roughness.
6. **A cutout in the prepass.** `shaders/prepass.wesl` never discards, so an
   alpha-masked leaf writes its whole rectangle into the depth that
   occlusion and refraction read. Done when a `mask` surface discards there.
7. **Layer folds in the inspector.** `editor/library/shaders/layers.wesl`
   brings each layer's knobs as `@if` features and `Params` fields;
   `walk_material` in `editor/scripts/inspector.bs` lists them flat. Done
   when each layer's knobs sit under a fold named for it.
8. **Decals and volumetric fog.** A `decal` component projecting a texture
   onto the depth buffer, and a froxel march for fog a light shafts through.
   Neither is in the fork; both read the shadow atlas.
9. **Global illumination in 3D.** Light that bounces, in real time, as a
   screen-space pass or an SDF probe field; nothing in the fork does it.

## Open questions

1. **Which PBR draws a node with no material.** It draws kiss3d's own
   material today; `shaders/pbr.wesl` is the surface a project material
   imports. Making `pbr.wesl` the built-in changes every scene's look.
2. **Sixteen or clustered for a project shader.** `mesh.wesl` binds no cluster
   buffers, so a project material sees `MAX_LIGHTS` lights while kiss3d's own
   material reads `cluster_max_lights`. Binding the clusters decides how much
   of the fork leaks into the contract.
3. **One layer vocabulary.** `light_layers` and `render_layers` are numbered.
   `docs/PLAN-rapier.md` step 1 names collision layers in the project, and a
   scene should not learn two sets of names.
4. **A probe taken back.** The fork's `ReflectionProbes` has no `clear`, so a
   removed probe keeps its array layer, shrunk to nothing.
