> **Status:** built. All six phases landed on 2026-08-31 —
> `crates/balaur_core/src/assets.rs` and the whole `crates/balaur_anim` crate,
> the `assets` and `animation` script modules (`balaur api`: 16 modules, 167
> functions), the editor's Animate persona rewired onto them, an animated
> platform in `examples/hello` and `examples/hello_rune`, and the
> ARCHITECTURE sections "Assets (core feature)" and "Animation (plugin
> feature)". Kept as written for the record; it is a plan, not a description
> of the code. See [generated/behaviour.md](generated/behaviour.md) for what
> the code actually does — roughly 150 of those sentences are this plan.
>
> **What §3.7 deferred stayed deferred**, deliberately and with nothing
> precluded: blend trees and state machines (the sampler is pure —
> `(clip, time) -> pose`, reachable with no `Engine` — so a blender composes
> samples later without the data model moving); skinning, rigs and
> retargeting (they need skinned meshes in the render backend first, and the
> track model already fits `bone/3/rotation`); and stable asset ids
> (`uid://`, GUIDs) — paths until renames actually hurt. Also not built:
> `tween_method` / `tween_subtween`, the `loop_finished` / `step_finished`
> signals, and asset hot reload through the file watcher (open question 4 —
> `assets.reload` is the mechanism a watcher would call, and wiring the
> watcher is a separate diff in a crate core does not own).
>
> **Where the implementation decided differently.** Twelve things, and each
> is argued at the code:
>
> 1. **No `animation_library` asset type.** §3.1 implied a second type for
>    named entries; instead a library file is addressed `hero.toml#idle` and
>    phase 1's entry resolution cuts the entry out, inheriting the document's
>    `type`. Core never learns the word `clips`. The cost is that a library
>    document is not itself a valid clip, which nothing needs it to be.
> 2. **A fourth reference form exists, written by nobody.** An inline
>    definition is keyed by a digest of its content and rewritten to
>    `#!<hex>`, so re-applying a component is idempotent rather than leaking
>    a cache entry per frame. It takes a `#entry` suffix (`#!<hex>#idle`) for
>    the same reason a file does — without it an inline library could not
>    autoplay, and the editor's **Make inline** could not be the inverse of
>    **Save as file**.
> 3. **`assets.load` returns the definition table, not the parsed object.**
>    `Value` has no variant for an opaque `Rc<dyn Any>` and adding one would
>    mean per-backend userdata in Luau and Rune — exactly what the seam
>    exists to avoid. The Rust `assets::load` still returns the shared `Rc`.
> 4. **`assets.save` was not built.** §2 listed it as the editor's write-back
>    path; the editor uses `fs.write` + `assets.reload`, which is already how
>    `model.save` writes a scene, and which has an answer for a packed run
>    (there is no writable source) where a core writer would not.
> 5. **An unresolvable reference warns rather than failing.** Component
>    `apply` errors are fatal to `instantiate_scene`, so one bad clip path
>    took a whole scene down. Godot logs a missing resource and loads the
>    scene; so does this. A definition *table* that will not parse is still
>    an error, reported where it was written.
> 6. **A method track is a `[[tracks]]` block with no `property`.** §3.1 gave
>    the key shape (`{ t = 0.8, call = "on_footstep" }`) but not the track.
>    Every candidate spelling puts a fake property into the closed set the
>    inspector reads; absence is honest, and a key that neither calls nor
>    carries a value is rejected naming which it needs.
> 7. **A key's `value` is one to four numbers, not always three.**
>    `shape/radius` is one and `color/rgba` is four, and the plan asks for
>    both. The track's width comes from its first key; transform tracks are
>    still fixed at three.
> 8. **`stop` ends a clip, `pause` holds it.** The plan lists both without
>    distinguishing them. `pause`/`resume` are meaningless unless one verb is
>    recoverable and the other is not — Godot's split.
> 9. **`seek` poses the node.** Phases 3 and 4 shipped it as playhead-only
>    and phase 5 made it pose, because scrubbing a *paused* clip otherwise
>    shows a moving ruler and a motionless node. It travels no span, so a
>    seek still cannot fire the method keys it skipped.
> 10. **`ease` bends the segment that arrives at its key**, and the 49th
>     spelling is bare `linear` (all 48 transition×mode names resolve too,
>     `in_linear` included, so a Godot port does not trip). Every curve maps
>     0→0 and 1→1 exactly, not to a tolerance.
> 11. **Tween start values are captured at the call**, not on the next frame
>     as Godot does, which makes a malformed spec an error where it was
>     written. A step's track holds its captured start value from t = 0 —
>     that is what "a tween is a generated clip" actually costs, and paying it
>     is what keeps the sampler with one set of semantics. `loops = 0` is
>     forever; repetition is the tween's business, not the generated clip's,
>     because a clip that never ends is a tween that never gets removed.
> 12. **Rust names:** `AnimationState` rather than `AnimState` (the crate name
>     is the only abbreviation §3.3 grants), and `tween::start` /
>     `tween::start_to` in Rust because `balaur_anim::stop` already exists.
>     The script API is `animation.tween` / `animation.tween_to` as specified.
>
> Two things the plan did not anticipate that the work required:
> `components::patch` grew a sibling for the merge (`defaults_of` + `overlay`,
> so `add` and `patch` share one implementation of shorthand and `#rrggbb`
> expansion), and the pose write had to become *deferred effects* — a
> component's `apply` may want the world mutably and a script handler may
> free its own node, so a step records `Patch`/`Call` and the frame carries
> them out once the borrows are gone.
>
> **`docs/NAMING.md` governs every name below.** This plan predates it, and
> the naming rules landed on 2026-08-30 with four rows aimed squarely at
> names proposed here: `AssetState` + `AssetTypeRegistry` rather than
> `AssetServer` (D1, N2), the `animation` script module with
> `animation.stop(id)` rather than `anim` and `anim.kill(id)` (D4, N1),
> `library = { type = "asset", asset = "animation_clip" }` rather than
> `kind = "asset", type = ...` (N6), and `assets.duplicate(path)` rather
> than `assets.instance(path)` (N1). All four are applied below. When a
> later edit to this plan disagrees with `docs/NAMING.md`, the naming file
> wins and this one is wrong.

# Plan: assets and animation

Two features, one plan, because the second needs the first. Animation clips
are the first thing Balaur has that several nodes want to *share* and that is
too big to inline in a scene node — which is exactly what Godot's `Resource`
and Unity's asset are for. Building animation without that layer would bake
clip data into scene files and force a migration later.

## 0. Where the tree is today

| | Status |
|---|---|
| Engine-side animation | **None.** No component, no system, no script module |
| Editor-side animation | `editor/scripts/anim.luau`, ~140 lines of Luau: one clip per node, position + euler keys, linear interpolation, Timeline dock scrubs and inserts |
| Shared data files | **None.** Everything a node needs is inline in the scene TOML; the only cross-file reference is `script = "scripts/x.luau"` |
| Asset/resource concept | **None.** `Pack::build` bundles every `.toml` it finds, so files exist, but nothing loads them by reference |

The editor writes what it edits: `model.save` encodes the whole document node,
so `[nodes.animation]` lands in the game's scene file. The runtime has no
handler for that key. Verified, headless:

```text
WARN balaur_core::project: scene key 'animation' on node 'Box' has no registered handler
```

So a clip authored in the editor is inert in the shipped game. That is the
hole this plan closes.

## 1. Prior art

### Godot

**Resources.** A resource is a data container with a path. It is either
*external* (its own `.tres`/`.res` file) or *built-in* (embedded in the
`.tscn` that uses it). The scene file declares both at the top and references
them by id:

```text
[gd_scene load_steps=3 format=3]
[ext_resource type="Texture2D" path="res://icon.svg" id="1_0f8yd"]
[sub_resource type="CircleShape2D" id="CircleShape2D_ab12"]
radius = 20.0
[node name="Sprite2D" type="Sprite2D" parent="."]
texture = ExtResource("1_0f8yd")
```

The distinction is decided at save time — clear a resource's path and it
becomes built-in; "Save As" promotes it to a file. `ResourceLoader` caches by
path, so two users of one path share one object; `duplicate()` and
`resource_local_to_scene` opt back out of sharing. Godot 4 adds `uid://` ids
so a file rename does not break references.

**Animation.** Three layers: an `Animation` resource (tracks, each with a
`NodePath` target, an interpolation mode and keys), an `AnimationLibrary`
resource (a named map of animations), and the `AnimationPlayer` node holding
libraries and playing `"library/animation"`. The player has `play`,
`play_backwards`, `queue`, `pause`, `stop`, `seek`, `advance`, `speed_scale`,
per-transition blend times, `autoplay`, `root_node` (what track paths resolve
against), and an `animation_finished` signal. `AnimationTree` sits on top for
state machines and blend spaces.

**Tween.** Created from a node (`create_tween()`), bound to it, killed with
it. Tweeners: `tween_property`, `tween_interval`, `tween_callback`,
`tween_method`, `tween_subtween`. **Sequential by default**; `parallel()`
makes the next tweener run with the previous one, `set_parallel(true)` makes
that the default, and `chain()` returns to sequential. Plus `set_loops`,
`set_speed_scale`, `set_trans` (12 transitions) and `set_ease` (4 modes),
`from()` / `from_current()` / `as_relative()`, and `finished` /
`loop_finished` / `step_finished` signals. Process mode selects idle or
physics tick.

### Unity

Every asset is a file with a `.meta` sidecar holding a GUID; references are
GUID + fileID, so sub-assets live inside a file and renames are free.
`ScriptableObject` is the "resource" analogue for custom data.

Animation is `AnimationClip` assets → an `AnimatorController` asset (state
machine, layers, blend trees) → an `Animator` *component* on the object.
Retargeting goes through an `Avatar` asset. The Playables API is the
code-driven graph underneath; Timeline is the sequencing tool above. The
legacy `Animation` component is roughly Godot's AnimationPlayer.

Unity ships **no** tween library; DOTween is the de-facto standard, and its
sequencing vocabulary is `Append` (sequential), `Join` (parallel with the
previous), `Insert(at, tween)` (absolute time).

### What to take, what to skip

| Mechanism | From | Take? |
|---|---|---|
| Data container, external file *or* inline in the scene | Godot | **Yes** — this is the whole ask |
| Path-keyed shared cache, opt-out copy | Godot / Unity | **Yes** |
| GUID / `uid://` indirection | Unity / Godot 4 | **No, v1.** Paths. Revisit when renames start breaking scenes |
| Named clip library in one file | Godot `AnimationLibrary` | **Yes** |
| Player as a node holding libraries, with autoplay/queue/speed | Godot | **Yes**, as a component (a Balaur node *is* its components) |
| State machine / blend trees | both | **Later tier.** Do not preclude: keep the sampler pure |
| Skeletal rigs, retargeting, avatars | both | **Later.** Needs skinned meshes in the render backend first |
| Tween: sequential by default, explicit parallel + chain | Godot + DOTween | **Yes** |
| Tween as a chainable object | Godot | **No** — see §3.4. Data form instead |
| Idle vs physics process mode | Godot | **Fixed tick only.** Determinism is non-negotiable here |

## 2. The asset layer

### Naming

`Engine::resource::<T>()` already means the engine's typemap
(`balaur_core::resources::Resources`). Calling the new thing "resource" puts
two unrelated concepts under one word in one crate.

**Decision: `assets` in Rust and in scripts** (`balaur_core::assets`,
`AssetState` for the cache, `AssetTypeRegistry` for the parser table, the
`assets` script module), with the docs saying plainly that this is what Godot
calls a Resource. `docs/NAMING.md` D1 has since ratified this and closed the
alternative: the typemap keeps `resource`, and renaming it to take the Godot
word is not on the table. The two type names come from N2's closed suffix
set — a path-keyed cache one subsystem owns and every writer reaches through
its own API is a `*State`; a table appended during plugin build and read-only
afterwards is a `*Registry`, exactly like `ComponentRegistry`. `Server` is
outside that set and is what this plan originally proposed.

### One rule for external vs inline

> **A string is a reference. A table is a definition.**

Every asset-typed field accepts either, which is the whole external/inline
duality in one sentence, and it generalises the `shorthand` idea that
`merge_defaults` already has.

```toml
# external file
[nodes.animation]
library = "animations/hero.toml"
autoplay = "idle"
```

```toml
# inline, anonymous: this node owns it, it is saved with the scene
[nodes.animation.clips.idle]
length = 2.0
loop = "loop"
tracks = [ { property = "position", keys = [ { t = 0, value = [0,0,0] }, { t = 1, value = [0,3,0] } ] } ]
```

```toml
# inline, shared by several nodes in one scene (Godot's [sub_resource])
[[assets]]
id = "hero_idle"
type = "animation_clip"
length = 2.0
tracks = [ ... ]

[nodes.animation]
clips = { idle = "#hero_idle" }
```

Reference syntax, three forms and no more:

| Form | Means |
|---|---|
| `"animations/hero.toml"` | the whole file |
| `"animations/hero.toml#run"` | the entry named `run` inside it |
| `"#hero_idle"` | an `[[assets]]` block in the same scene file |

### Loading

- `AssetState`, an engine resource: `DetHashMap<Key, Rc<Asset>>`, so two
  nodes naming one path share one parsed clip and iteration stays
  deterministic.
- Typed loaders are registered by plugins, exactly like components:
  `app.register_asset_type("animation_clip", parse_fn)`, which appends to an
  `AssetTypeRegistry`. `balaur_anim` registers one; nothing in core knows
  what a clip is.
- Source resolution costs **no new core plumbing**:
  `ScriptHost::scene_source(rel)` already resolves project-relative TOML from
  the pack in packed runs and from disk otherwise, and `Pack::build` already
  bundles every `.toml`. Fall back to `ProjectRoot` + `std::fs` when no script
  host is running (Rust-only apps and tests).
- Sharing is the default; `assets.duplicate(path)` returns a private copy
  (Godot's `duplicate()` / `resource_local_to_scene`).
- Hot reload: the file watcher is script-only today. Extend it to `.toml`
  assets — reparse, bump a generation counter, players re-resolve on the next
  frame. The editor calls `assets.reload(path)` after writing.

### Script API

```luau
local clip = assets.load("animations/hero.toml#run")   -- table, cached
assets.save("animations/hero.toml", clip)              -- editor writes back
assets.reload(path);  assets.exists(path);  assets.duplicate(path)
```

### Editor, for free

The inspector is generated from component schemas, so the schema vocabulary
gains one type:

```toml
library = { type = "asset", asset = "animation_clip", default = "" }
```

`type` is the datatype key (`docs/NAMING.md` N6), so the asset's own type name
needs a second key rather than reusing it. N6's closed set is now enforced:
`parse_schema` panics at registration on a `type` outside
`balaur_core::components::PROPERTY_TYPES`, so phase 1's first task is adding
`asset` to that const and an arm to the default checker (an asset default is
a path string, so it shares the `string` arm). Until then the schema line
above fails at boot rather than at the first inspector row — which is the
closed set working, not a bug.

One new type buys a picker row, plus the two buttons that make the model
legible — **Save as file** (inline → external) and **Make inline** (external →
inline copy) — for *every* asset type anyone ever registers.

## 3. Animation

### 3.1 Clips

```toml
type = "animation_clip"
length = 2.0
loop = "loop"                 # none | loop | pingpong

[[tracks]]
target = ""                   # node path relative to the player; "" = self
property = "position"         # position | rotation_euler | scale | <component>/<prop>
interp = "linear"             # step | linear | cubic
keys = [
  { t = 0.0, value = [0, 0, 0] },
  { t = 1.0, value = [0, 3, 0], ease = "out_back" },
]
```

A library file is the same document with named entries (`[clips.idle]`,
`[clips.run]`), addressed as `hero.toml#run`.

**Property addressing reuses the component registry.** `components::get` /
`add` are generic over every registered component, so `color/rgba`,
`shape/radius` and `widget/x` animate without `balaur_anim` ever depending on
`balaur_render` or `balaur_ui` — and a third-party plugin's components animate
the day they are registered. Core gains one function for this:
`components::patch`, which merges over the *current* value; today
`set_component` merges over schema defaults, so animating `shape/radius`
would silently reset `half_extents`.

Rotation keys are authored as euler (matches `set_rotation_euler`, readable in
a diff) and slerped as quaternions. The editor's current euler lerp flips past
±180°.

Method tracks: `{ t = 0.8, call = "on_footstep" }` dispatches through
`ScriptHost::call_on`.

### 3.2 The player

```toml
[nodes.animation]
library = "animations/hero.toml"
autoplay = "idle"
speed = 1.0
root = ""                     # what track targets resolve against
```

All scalars, so the generated inspector renders it with no editor code.

### 3.3 Runtime API

The module is `animation`, not `anim`: every existing module is a full word
or an established initialism (D4 bans abbreviations in the script API), and
the scene key this plan already writes is `[nodes.animation]`. Stopping is
`animation.stop`, for a playing node and for a tween handle alike — `kill`
would be a third destruction verb beside `node.queue_free` and
`physics.clear` (N1). Only the crate stays `balaur_anim`, which is
rust-internal and reads against the other `balaur_*` crate names.

```luau
animation.play(node, "run", { speed = 1.5, from_start = true })
animation.queue(node, "idle")
animation.stop(node);  animation.pause(node);  animation.resume(node);  animation.seek(node, 0.4)
animation.current(node);  animation.time(node);  animation.is_playing(node)
animation.just_finished(node)      -- clip name, this frame only
animation.define(node, "hurt", { length = 0.4, tracks = { ... } })
```

Finished delivery is `on_animation_finished(name)` via `call_on`, plus the
`just_finished` query for a script that would rather poll than declare a
method. When this was written `call_on` carried no arguments, so the handler
took none and the query was the only way to learn *which* clip ended; the seam
gained arguments afterwards (2026-08-31) and the handler now takes the name.
`Value::Callback` is still valid only during the binding call that received
it, so there is still no `connect`-style subscription — a named method with a
payload is the seam's answer, and it needs no id space and no release.

### 3.4 Tweens are generated clips

**The unification worth having:** a tween is a short clip whose start values
are captured when it starts (Godot's `from_current`). One sampler, two
authoring front-ends. It also makes tweens serialisable — so the editor can
author them, and they hot reload — which no engine surveyed here offers.

```luau
local id = animation.tween(self.node, {
  loops = 1, speed = 1.0,
  steps = {
    { property = "position",   to = {0, 3, 0},    duration = 0.5, ease = "out_back" },
    { property = "color/rgba", to = {1, 0, 0, 1}, duration = 0.5, parallel = true },
    { interval = 0.2 },
    { call = "on_landed" },
    { property = "position", by = {0, -3, 0}, duration = 0.4, ease = "in_quad" },
    { target = "Shadow", property = "scale", to = {2,2,2}, duration = 0.4, parallel = true },
  },
})
animation.stop(id);  animation.is_running(id)
```

- **Sequential by default.** `parallel = true` joins the step to the previous
  one (DOTween's `Join`, Godot's `parallel()`); the next non-parallel step is
  Godot's `chain()`. Two words cover both engines' vocabulary.
- `to` absolute, `by` relative (`as_relative`), `from` explicit; default start
  is the current value.
- `target` tweens another node from the same call.
- Tweens die with their node.
- Sugar for the 90% case: `animation.tween_to(node, "position", {0,3,0}, 0.5, "out_back")`.

**Why not Godot's chainable builder.** `create_tween().tween_property(...).set_ease(...)`
needs a handle object with methods, which means new userdata in *every*
backend (`UserDataRef` in Luau, protocols in Rune) and again in every future
language. That is precisely what `node_api::NODE_OPS` exists to avoid: the
seam's rule is declare once, reach every language. The data form needs zero
backend sugar — and a backend may add builder sugar over it later, exactly the
way it already adds `node:position()` sugar over the declaration list.

### 3.5 Easing

12 transitions × 4 modes, named `linear`, `in_quad`, `out_back`,
`in_out_elastic`, matching Godot's set so ported curves behave.

Implemented on the `libm` crate — already a workspace dependency, already the
determinism fix in `balaur_script_luau::det`. `f32::sin`/`powf` route to the
platform libm and differ across OSes, which would put a per-platform drift
into any simulation an animation touches.

### 3.6 Scheduling

Advance on a fixed 1/60 accumulator inside `AnimState`, the same shape as
`PhysicsState` — *not* on `engine.time()` the way the editor prototype does,
which leaks variable dt into simulation.

System stage: `Stage::Update`. Systems registered by plugins run after the
script tick (`App::new` registers the script system first), so `animation.play()`
takes effect the same frame; and physics reads `Transform` for kinematic
bodies in `PostUpdate`, so animated moving platforms push things correctly
with no extra wiring.

Later, and visual only: interpolate the leftover accumulator at render time.
It must never feed back into simulation state.

### 3.7 Deliberately not now

- **Blend trees and state machines** (AnimationTree, AnimatorController). Keep
  the sampler pure — `(clip, time) -> pose` — so a blender composes samples
  later without touching the data model.
- **Skinning and rigs.** Needs skinned meshes in the render backend. The track
  model already fits (`bone/3/rotation`).
- **Stable asset ids** (`uid://`, GUIDs). Paths until renames actually hurt.

## 4. Phases

| # | What | Exit |
|---|---|---|
| 1 | Asset layer in core: `assets.rs`, `AssetState` + `AssetTypeRegistry`, `[[assets]]` in scene docs, the string-vs-table rule, the `assets` script module, the `asset` schema type (one entry in `PROPERTY_TYPES`) | A component resolves an external ref, a `#id` ref and an inline table; a packed run behaves identically to a dev run |
| 2 | `balaur_anim`: clip asset type, sampler, `animation` component, fixed-step system | Headless test animates position/rotation/scale, twice, bit-identically; the editor-saved `[nodes.animation]` stops warning |
| 3 | `animation` script module, component-property tracks, `components::patch`, method tracks, `on_animation_finished` | A Luau *and* a Rune script drive the same clip; `color/rgba` animates with no render dependency in `balaur_anim` |
| 4 | Tweens over the same sampler: steps, parallel/chain, loops, handles, the `libm` ease library | Sequential, parallel and mixed tweens land on the exact same frame twice in a row |
| 5 | Editor rewire: Timeline and the Animate persona edit an engine clip through `assets`; save-as-file / make-inline | What the editor previews is what ships; most of `anim.luau` deletes |
| 6 | Example + docs: an animated platform in `examples/hello`, ARCHITECTURE section, regenerated `docs/generated/`, README bullet | `scripts/ci.sh` green |

```text
  1 assets ─┬─ 2 clips ── 3 script API ── 4 tweens ─┬─ 5 editor
            │                                       └─ 6 docs
            └─ (also unblocks: prefabs, audio assets, materials)
```

Phase 1 is the one with leverage beyond animation: roadmap items 3 (prefabs /
sub-scenes) and 4 (assets in packs with content hashing) both need exactly
this layer, and a `PackedScene`-style prefab is then just another asset type.

## 5. Open questions

1. ~~**`assets` vs renaming the typemap to take the word `resource`.**~~
   **Settled** by `docs/NAMING.md` D1 (2026-08-30): the typemap keeps
   `resource`, content is `assets`, and the collision never happens. The
   measured cost of the alternative was ~170 compiler-caught sites to buy a
   noun. `Server`, `Manager` and `Handler` are denied as typemap suffixes at
   the same time, which is why the cache is `AssetState`.
2. **Signals.** ~~`call_on` carries no arguments~~ — **settled 2026-08-31**:
   `ScriptHost::call_on(node, method, args)` now carries a payload in both
   backends, and `on_animation_finished(name)` uses it. Script callbacks are
   still call-scoped, so subscription-style signals would still need an id
   space with explicit release; nothing has needed one.
3. **Animating a dynamic body fights physics** (as it does in Godot). Warn
   once, document, and let it be.
4. **Asset hot reload touches the watcher**, which is script-only today.
   Phase it after 3 if the diff gets wide.

## Sources

- [Tween — Godot docs](https://docs.godotengine.org/en/stable/classes/class_tween.html)
- [Resources — Godot docs](https://docs.godotengine.org/en/stable/tutorials/scripting/resources.html)
- [AnimationPlayer — Godot docs](https://docs.godotengine.org/en/stable/classes/class_animationplayer.html)
- [TSCN file format — Godot docs](https://docs.godotengine.org/en/stable/engine_details/file_formats/tscn.html)
- [Animation overview — Unity manual](https://docs.unity3d.com/Manual/AnimationOverview.html)
