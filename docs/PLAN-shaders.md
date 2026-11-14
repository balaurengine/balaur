# Plan: shaders

What is left of the shader system in `crates/balaur_render`. WESL shaders,
the `material` asset, post-process materials and shaders linked at export are
built.

## Steps

1. **A shader per dimension.** A material names `shader` and `shader_2d` in
   one file, sharing one `[params]` table, since group 3's layout is already
   shared. One material then styles a mixed subtree through the `material`
   component. `shaders::contract` already reads which dimension a shader
   draws, and `shaders::fits` keeps the built-in where it does not. Done when
   one asset draws both a `mesh` and a `sprite` under one `material` node.
2. **A shader graph.** A graph on the node canvas
   `docs/PLAN-authoring-without-code.md` brings, emitting readable WESL. A
   node is a function in a module, a graph is imports and calls, and
   `[params]` are read off the linked shader as for any other. Done when a
   saved graph writes a `.wesl` a material links and a user can take over.

## Open questions

1. **Where a param's range and description live.** `render::material_params`
   derives the inspector's rows from the shader's `Params` struct, and a
   `Field` carries only a name, a type and an offset. A range needs the
   material or the shader to annotate the struct.
2. **Whether kiss3d exports its modules.** It links its built-ins in its
   `build.rs`, so Balaur's `shaders/common.wesl` keeps its own copy of
   helpers such as `unpack_mat3`.
3. **A crates.io package of Balaur's helpers.** Waits for someone to ask; a
   project's own module already mounts through
   `shaders::register_shader_module`.
