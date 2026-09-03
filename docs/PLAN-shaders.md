> **Status:** phases 1 and 2 built on 2026-09-02. The engine's shaders are
> WESL, linked through `balaur_render::shaders`; the `material` asset parses,
> links and packs its values (`balaur_render::material`); `sprite` takes a
> `material`, and `balaur_render::shader_material` draws it as a kiss3d
> `Material2d`. Phase 3 is built too: `mesh` and `shape3d` take a `material`,
> and `shader_material_3d` draws it with the scene's lights and fog through
> `package::mesh`. Phase 4 is built: saving a shader relinks it, and
> a link error names the file, line and column the author wrote, which
> `render::check_material` hands to the editor's Problems list;
> `render::material_params` gives the inspector a row per field the shader
> declares, written straight back to the material file; and a `.wesl` opens
> in the code editor, highlighted as WESL, from the material that names it.
> Phase 5 came with the asset: `[features]` picks a variant at link time.
> Phase 8 is built — WESL validation catches a call to a name nothing
> declares, and `shaders::eval_floats` runs a `@const` shader function on the
> CPU, so the lighting is asserted on headless. Phase 7's rewrite is built
> (`balaur_render::preview`); drawing it and reading a texel back are not.
> Phase 9's plugin half is built: `shaders::register_shader_module` mounts a
> plugin's module beside the engine's own, so a project's shader imports it
> like any other. Phase 6 is built: `render::set_channel` draws normals, UVs,
> depth or the bare texture instead of the picture, in 2D and 3D, from the
> command palette. Phase 7 draws: clicking a shader's gutter previews that
> line's value. Its *number* is not built — see open question 8 — and
> publishing the helpers as a crates.io package is a release action.
>
> **Where the implementation decided differently:**
>
> 1. **Linking happens at run time, not in `build.rs`.** Phase 1 called for
>    `build_artifact` + `include_wesl!`. A project's shaders have to link at
>    run time regardless, and kiss3d already links its own that way, so one
>    path serves both instead of two that can disagree.
> 2. **`wesl` is not behind the `kiss3d` feature.** Linking is CPU work with
>    no GPU in it, and CI runs `cargo test --workspace` without the windowed
>    backend — behind the feature, no test of a shader would ever run. The
>    same reasoning `image` carries in that `Cargo.toml`.
> 3. **A param the shader does not read warns rather than errors.** Stripping
>    removes a field the shader stopped reading, so commenting out a line
>    would otherwise fail every scene using the material.
> 4. **A shader's contract is a WESL module, not boilerplate.** The uniforms,
>    the vertex inputs and the transform live in `package::sprite`, mounted
>    for free, so a project's shader is its own two entry points and nothing
>    else. This is what the plan meant by a `Params` struct "at a known group
>    and binding" — the binding is in the contract, not in prose.
> 5. **`polygon` takes no material.** It already draws through the skinning
>    material, which owns its own buffers; one node cannot have two. `sprite`
>    and `shape2d` are kiss3d's own geometry and take one.
> 6. **Shader files ride in the pack's text map.** `scenes` was already
>    holding asset documents as well as scenes; `.wesl` joins them rather
>    than growing a fourth map for one file type.
> 7. **Hot reload rides the asset generation, not a second watcher.**
>    `assets::generation` already exists for exactly this, so a saved `.wesl`
>    bumps it (`assets::invalidate`) and the backend's material cache clears
>    on the change — one signal, not a watcher per subsystem.
> 8. **Errors name the file, not the module.** WESL spans point at the module
>    path the linker was handed, which is a name this code invented;
>    `compile` rewrites it to the shader path so a span is somewhere an
>    editor can put a marker.
> 9. **A material is registered with kiss3d, not only attached to the node.**
>    `begin_frame` — and every per-frame capability the window supplies —
>    goes through `MaterialManager*`'s own materials. A material that is only
>    attached never gets it, and writes its view and clock once and then
>    never again.
> 10. **kiss3d only offered those capabilities to its own material.** Image
>    based lighting, reflection probes, SSAO, the transmission background and
>    the clustered light buffers were all sent to `get_default()`, so a
>    material a game registers could never receive them however it
>    implemented the trait. The fork broadcasts them through a new
>    `MaterialManager3d::for_each`. What Balaur's 3D material *binds* today is
>    the fixed-light path, fog and the tint; the other five arrive but are not
>    yet read, and each is another entry in the frame group rather than a new
>    group, because WebGPU guarantees only four.
> 11. **Param rows reuse the component vocabulary.** `material_params`
>    answers in the type names a component schema uses — `float`, `vec2`,
>    `vec3`, `color` — so the inspector draws them with the editors it
>    already has. A `vec4` is reported as a colour: it is what one nearly
>    always is, `Value::Color` is the engine's own four-channel type, and
>    `[params]` takes `#rrggbb` and `[r, g, b, a]` alike either way.
> 12. **A shader opens in the code pane, not a pane of its own.** `S.shader_rel`
>    aims the existing editor at a `.wesl`; the hooks panel and the
>    breakpoint gutter stand down, because neither means anything in a
>    shader, and saving writes the file rather than rebuilding a unit.
> 13. **Validation catches names, not types.** With wesl's `eval` feature on,
>    `validate: true` rejects a call to a function nothing declares — which
>    otherwise reaches naga and so needs a GPU to find. It does not check
>    types: a `vec4` function returning `1.0` still links. naga stays the
>    type checker.
> 14. **`@const` is stripped on the way out, not at link.** The attribute is
>    what lets `eval_floats` call a shader function, so it has to survive
>    linking; WGSL has no such thing, so `shaders::wgsl` drops it from what a
>    backend compiles. Stripping it earlier makes the evaluator refuse its
>    own helpers.
> 15. **The preview is a derived source, not `@if(debug)` in the author's
>    file.** The plan put the channel behind a feature flag; a rewrite that
>    produces a second source is simpler, leaves the file alone and needs no
>    flag, and the channel is what ships nowhere.
> 16. **The preview needs the type written.** WESL's type checker is not
>    exposed (`wgsl-types` has it, `wesl` does not re-export it), so
>    `preview` reads the type off an annotation or a constructor and says
>    what to write when there is neither. Guessing would draw a shader that
>    does not compile.
> 17. **A plugin's shader modules are a registry, not a package.** Phase 9
>    imagined crates.io packages; a plugin already in the process only needs
>    its source mounted, which `register_shader_module` does. Publishing is a
>    release action, and a separate one.
> 18. **A channel view is a material swap, not a second pass.** kiss3d's AOV
>    renderer draws into its own targets, which would then have to be
>    composited; the materials built for phase 2 and 3 already draw whatever
>    a shader says, so a channel is one more shader and no new pass.
> 19. **The preview is a gutter click, not the caret.** `ui::code_editor`
>    reports a gutter click and not a caret line, and a shader's gutter holds
>    no breakpoints, so the gesture was free. It is also the better one: a
>    caret moves constantly and relinking on every keystroke would be worse
>    than asking.
> 20. **2D has no room for a fifth group either.** The sprite material already
>    uses frame, object, texture and params. When `docs/PLAN-rendering.md`
>    adds 2D lights they fold into the frame group; they cannot take a group
>    of their own.

# Plan: shaders and materials

## Where the tree is today

- Balaur owns exactly one shader: `const SHADER` in
  `crates/balaur_render/src/skinned_2d.rs:71`, a WGSL string handed to
  `Context::create_shader_module`. Everything else a scene draws goes
  through kiss3d's built-in materials.
- A renderable's look is two fixed knobs: `color` and, where it applies,
  `texture`. No component has a material, and there is no `shader` or
  `material` asset type.
- kiss3d 0.46 already composes its own shaders with **WESL**: `common.wgsl`
  mounted as `package::common`, `@if(skinned)` variants in the shadow
  passes, dead-code stripping, and `wesl::VirtualResolver` doing the
  linking in `builtin/mod.rs`. `wesl` 0.4.4 is in `Cargo.lock` already —
  it is why `rust-toolchain.toml` pins 1.98.
- Rendering is an observer. Nothing a shader computes may reach the tick,
  and that does not change here.

## What WESL is

WESL (`https://wesl-lang.dev`, spec 0.2, `wesl` 0.4.4, MIT/Apache-2.0) is a
strict superset of WGSL: every `.wgsl` file is already a valid WESL file.
It adds the four things a shader library needs and WGSL has none of:

| Feature | What it buys |
| --- | --- |
| `import package::common::luminance` | One definition of a helper, not one copy per pass |
| `@if(skinned)` / `@elif` / `@else` | Variants from one source, chosen at link time |
| Stripping (dead-code elimination) | A variant's output carries only what its entry points reach |
| Cargo and npm packages | Shader libraries a game or an extension can depend on |

The compiler is a Rust crate that emits **plain WGSL**, so nothing
downstream changes: naga still validates, wgpu still compiles, the web
export still gets the language WebGPU speaks natively. It links either in
`build.rs` (`build_artifact` + `include_wesl!`) or at run time
(`Wesl::new(dir).compile(&path)`), over a `FileResolver` for a directory
or a `VirtualResolver` for sources held in memory — which is what a game
reading shaders out of an export pack needs.

Two caveats worth writing down now. WESL's own validation sits behind the
`eval` crate feature, off by default, so today naga is the validator and
its spans point at the *linked* output; `use_sourcemap(true)` plus
`Diagnostic::with_sourcemap` is what maps a span back to the file and line
the author wrote. And `wgsl-analyzer`'s WESL support is still landing, so
editor completion for a `.wesl` file is behind the language itself.

## The shape

A `material` asset, following the rule every other asset follows — a
string is a reference, a table is a definition:

```toml
[nodes.sprite]
texture = "art/water.png"
material = "materials/water.toml"     # or the table, written in place

# materials/water.toml
shader = "shaders/water.wesl"
features = { lit = true }             # @if flags, chosen at link time
[params]
speed = 0.4
tint = "#3aa0ff"
```

`material` is an `asset`-typed property on `sprite`, `shape2d`, `polygon`,
`tilemap`, `shape3d` and `mesh`, which is what gives it an inspector row
for free. Empty means the built-in material, so every scene that exists
keeps drawing what it drew.

The shader itself is ordinary WESL with one contract: a `Params` uniform
struct at a known group and binding, whose fields are the `[params]`
table. Balaur links it with `VirtualResolver` over the project's
`shaders/` directory plus its own `package::balaur` module — the frame
uniforms, the model matrix, the helpers — and hands the WGSL to a
`ShaderMaterial` implementing kiss3d's public `Material2d` and
`Material3d`.

A material is an asset like any other: shared by default, parsed headless
(so a scene loads on a machine with no GPU), and compiled only when a
backend that draws asks for it.

## Determinism

Unchanged, and worth being explicit about because user-authored code is
new here. A shader is an observer: it reads uniforms the frame wrote and
writes pixels. `params` are scene state and go in the digest as any
component property does; the *values a shader computes* never come back.
Time in a shader is the render clock, not the tick counter, for the same
reason a screenshot is not simulation state.

Headless is the test of this: a headless run parses every material, links
nothing, draws nothing, and produces the same digest as a windowed one.

## Two files, not one

A shader is source; a material is data. The distinction is the one the
tree already draws between a `.rn` script and a component:

| | Lives in | Edited in | Reloads on |
| --- | --- | --- | --- |
| `shaders/water.wesl` | The project, beside scripts | The code editor, gutter and all | Save |
| `materials/water.toml` | An asset, shared or inline | The inspector | Write |

Only the material is an asset type. The `.wesl` file is named by one, the
way a `script` component names a `.rn` file, so nothing needs a second
mechanism to find it, watch it or ship it.

## Editor

- **The material row comes free.** `material = { type = "asset", asset =
  "material" }` on a component is drawn by `edit_asset` in
  `editor/scripts/inspector.rn:647` — the path field, and the
  inline/save-as-file pill that every asset row has. Nothing to write.
- **Params are rows of the types the inspector already draws.** `speed` is
  a float slider, `tint` a colour swatch, `mask` a texture path — dragged
  while the game runs, because a param is a uniform and a uniform is
  written every frame.
- **Features are checkboxes.** A material's `@if` flags are booleans;
  toggling one relinks the shader, which is the same path a save takes.
- **The shader opens in the code editor.** `ui::code_editor` already takes
  a keyword palette per language (`k_key`, `k_str`, `k_fn`, …); WGSL is
  another keyword set, not another editor.
- **Errors land where script errors land.** A failed link or a naga
  validation failure is reported with the file and line the WESL sourcemap
  gives back, highlighted in the gutter the way a paused script line is. A
  bad shader keeps the last good compile, or falls back to the built-in
  material — never a blank viewport.
- **Hot reload.** Saving a `.wesl` relinks the materials naming it, riding
  on asset hot reload in `docs/PLAN-scenes-and-assets.md`.

## Debugging a shader

There is no breakpoint. No GPU API has one — not wgpu, not WebGPU — so
`docs/PLAN-debugger.md`'s model (park the VM, read the frames) has no
analogue on the device. What replaces it, in the order the loop actually
uses them:

1. **Hot reload is the debugger.** Save, see it. Sub-second iteration is
   what makes shader work tractable, and it is phase 4 above.
2. **Channel views.** kiss3d already renders auxiliary outputs —
   `AovKind::{Depth, Normals, CameraNormals, Segmentation}`. A viewport
   dropdown that draws normals, depth, UVs or overdraw instead of colour
   answers most "why is it black" questions without touching the shader.
   A material can expose one of its own values on the same channel under
   `@if(debug)`.
3. **Value preview at the cursor.** Put the caret on a line and the
   viewport draws the value that line computed, for every pixel. This is
   the closest thing to a breakpoint a shader has, and it has its own
   section below.
4. **A CPU probe, if the interpreter holds up.** WESL's `eval` feature
   carries a WGSL interpreter: `CompileResult::exec(entrypoint, inputs,
   bindings, overrides)` runs one invocation on the CPU and hands back the
   return value and every writable binding. It is marked highly
   experimental and does not cover every builtin. The preview below gets
   the same numbers off the GPU with far less standing on experimental
   ground, so this is a curiosity for rendering and a real tool for tests.
5. **Capture tools still work.** A wgpu app is a RenderDoc, PIX and Xcode
   frame-capture app; a `--capture` flag that names the file is a
   half-day, and gives every question the above cannot answer.

## Value preview at the cursor

Godot arrived here too: the Shader Previewer addon (now upstream) shows
the visual state of a variable at the line the caret sits on. Same idea,
built on the AST rather than on text.

The naive version is truncation — insert `return vec4(expr, 1.0)` after
the statement at the caret and let stripping delete everything after.
It works, and it only works in the entry point: a caret inside a helper
function has nowhere to return to, and a caret inside an `if` says
nothing about the pixels that took the other branch.

The version worth building writes to a channel instead of returning:

```wgsl
@if(debug) var<private> dbg: vec4f;
@if(debug) var<private> dbg_hit: bool;

// injected where the caret is, at any depth:
@if(debug) { dbg = vec4f(speed, 0.0, 0.0, 1.0); dbg_hit = true; }

// injected at the entry point's return:
@if(debug) return select(color, dbg, dbg_hit);
```

Which buys three things truncation does not. It works inside a helper,
because a global outlives the call. It works inside a branch — and
`dbg_hit` is itself the answer to "which pixels reach this line", drawn
as a mask. And the shader still runs to completion, so a pass with more
than one output still writes them.

**The mechanics are already in the tree.** `wgsl-parse` 0.4.4 is in
`Cargo.lock` — it is WESL's own parser. Statements are `Spanned<Statement>`
carrying byte ranges, and `TranslationUnit` implements `Display`, so the
AST round-trips to source. Parse, find the statement whose span holds the
caret, splice two statements in, print, link. No regex, no text munging.
The whole scaffolding is behind `@if(debug)`, so stripping deletes it from
the variant that ships — which is the specific thing WESL buys here.

**Encoding the value is the actual design work.** A preview is a picture
of a number, and the number has a type:

| At the caret | Drawn as |
| --- | --- |
| `f32` | Greyscale, with a range control |
| `vec2` | Red and green |
| `vec3` | Straight to RGB |
| `vec4` | RGB, with alpha on a toggle |
| `bool` | The mask |
| `i32` / `u32` | A palette, or a ramp |

A float in 0..1 and a float in 0..1000 are both a flat image without a
range control, so raw / abs / normalized and an exposure multiplier sit
beside the preview, not in a menu.

**And then the number itself.** With the value in a render target,
hovering a pixel and copying back that one texel gives the float under the
cursor in a tooltip — a 1×1 readback, no interpreter, no `eval` feature.
Real GPU, real textures, real interpolation, every pixel at once, and
exact digits for the one being pointed at.

A vertex-stage caret needs the value carried to the fragment stage as a
`@if(debug) @location(n)` varying — the same injection, one more site.

**What it costs.** One link and one pipeline per caret position, cached by
position, so moving down a function is one compile per line and instant
on the way back up. A value that depends on a binding the pass does not
have cannot be previewed, and says so.

## Shader tests

The same interpreter is worth more in CI than in the editor. Balaur's
tests are headless and its runners have no GPU, so today a shader cannot
be tested at all — a broken one is caught by a human looking at a window.
With `exec`, a shader function is testable the way a Rust function is:
feed it inputs, assert on the output, no device involved. `srgb_to_linear`
round-trips, a lighting term is zero behind the surface, a UV wrap lands
where it should. That is the first test file the `eval` feature buys, and
it is the argument for turning the feature on.

## Phases

1. Balaur's own shaders move to `.wesl` files with a `package::balaur`
   module of shared helpers, linked in `build.rs`. `skinned_2d` is the
   first and, today, the only one. Nothing user-facing changes; this is
   the step that proves the toolchain in CI and on every platform the
   editor ships to.
2. The `material` asset and the `material` property on the 2D
   renderables: runtime linking, the `Params` uniform, the fallback on
   error.
3. The same for 3D — `mesh` and `shape3d` — over `Material3d`.
4. Editor: hot reload on save, sourcemapped errors in the gutter, param
   rows, WGSL keywords in the code editor.
5. `features` on a material, so one shader source serves variants; the
   lit-2D work in `docs/PLAN-rendering.md` is the first consumer, since a
   lit and an unlit sprite are `@if(lit)` apart.
6. Channel views in the viewport over kiss3d's AOVs.
7. Value preview at the caret: the AST injection, the encodings and the
   range control, then the texel readback that puts a number on it.
8. The `eval` feature: shader tests that run headless, and validation
   before naga sees the output.
9. Post-process materials on `camera.post`, and shader packages —
   Balaur's helpers published as one, and an extension able to add its
   own.

## Open questions

1. **Where the param schema comes from.** Either the material declares it
   (`speed = { type = "float", default = 0.4 }`, like a component schema)
   or Balaur parses the shader's `Params` struct — `wgsl-parse` is
   already in the tree — and derives the rows. The second is less to
   write and cannot drift from the shader; the first is where a range or
   a description would have to live. Probably both: parse the struct,
   let the material annotate it.
2. **When linking happens in an export.** Linking at load costs
   milliseconds per material on a cold start; linking at export time puts
   WGSL in the pack and drops the compiler from the shipped binary.
   Export-time is the answer, once `--export` learns to walk materials.
3. **Whether kiss3d should expose its linker.** `compile_wesl` and
   `package::common` are `pub(crate)`. If they were public, Balaur's
   shaders would import the engine's helpers instead of keeping a second
   copy. Worth an upstream ask before writing the second copy.
4. **Validation without a GPU.** With the `eval` feature on, WESL
   validates before naga does, which is the only way a headless CI job
   catches a broken shader. It is a heavier dependency; the call is
   whether the check is worth it in the editor build alone.
5. **How far the interpreter goes.** It is 4,000 lines and honest about
   being experimental. Shader tests need only the pure functions and are
   worth trying early; a CPU probe would need textures, samplers and
   interpolated inputs, and may simply not be there yet. Phase 8 is what
   tells us which — and the caret preview means the answer no longer
   gates a debugging story.
6. **A shader graph.** Not planned, but the module system is what would
   make one honest: a node is a function in a module, a graph is imports
   and calls, and the output is readable WESL a user can take over. Every
   graph built on string concatenation regrets it. If it is ever built,
   it is built on this.
8. **The preview's number needs a buffer, not the frame.** Reading a pixel
   back through `snap_image` gives the *tonemapped 8-bit* colour, not the
   value the shader computed — a plausible-looking number that is wrong.
   An exact read means the preview writing to a storage buffer, and the
   material's four bind groups are already spent (frame, object, texture,
   params), so it waits on the group budget being reworked.
9. **Compute shaders.** Out of scope. A compute pass that wrote back into
   simulation state would break the observer rule; one that only feeds
   rendering (particles on the GPU) is a later plan, and needs the same
   headless answer particles already have.
