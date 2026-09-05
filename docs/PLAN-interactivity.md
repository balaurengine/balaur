> **Status:** not started. Written down on 2026-09-05 from the Spline
> comparison: its whole authoring model is states, events and actions, and
> Balaur's answer to all three is a script. The order is what unblocks the
> most: pointer hooks first, because a designer's first interaction is a
> hover and nothing fires one on a drawn node today; then states, because a
> transition between two looks is the second; then variables and bindings,
> which are the glue; then the camera and control rigs, which are presets
> over things that exist. None of this is a second runtime: every binding
> resolves to a call a script could make, and "convert to script" writes it.

# Plan: interactivity without a script

Pointer, key, action and resize hooks on any node; states with transitions;
scene variables; event bindings a designer edits in the Events view; and
camera and control rigs shipped as presets. The Events view itself is
`docs/PLAN-editor-ergonomics.md`; the page-side half of variables is
`docs/PLAN-embed.md`.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| Hooks dispatched to a node's script: collisions, contact force, joint break, tween and animation finished, focus, `on_click` on widgets, web and network messages | `balaur_physics`, `balaur_anim`, `balaur_ui`, `balaur_web`, `balaur_http` |
| An event bus: `emit`, `subscribe`, `emitted`, `on_event` | `events` |
| Headless picking: the ray through the mouse, the nearest node it meets | `render.mouse_ray`, `render.pick_ray`, `pick.rs`; `render.mouse_world_2d` for 2D |
| Point and shape queries in both physics worlds; sensors that fire `on_collision_start` | `physics2d`, `physics3d`, `collider*.sensor` |
| Tweens with easing, delay, chaining; clips with method tracks; `tween_to` on component paths | `animation`, `balaur_anim` |
| A patch verb on components, and prefab overrides addressing `<component>/<property>` | `node.patch_component`, prefabs |
| Input actions with rebinding, gamepads, touch, scroll | `input` |
| Persisted settings and save slots | `settings`, `save` |
| HTTP, websockets, Gamend, `engine.open_url`, `scene.instantiate`, `scene.spawn`, `node.free`, `camera.current`, `sound`, `particles.emitting` | the respective modules |
| Character controllers in 2D and 3D | `character2d`, `character3d` |
| `exports()` spec tables: typed, ranged, ordered | `docs/PLAN-scripting-nodes-ui-editor.md` §1.2 |
| An Events view and a hooks sheet that list the hooks a file declares | `editor/scripts/center.rn`, `highlight.rn` |
| Every input recorded, so a session replays bit for bit | `replay` |

Missing:

- **Pointer hooks on drawn nodes.** A script that wants hover polls
  `pick_ray` itself every tick. Nothing fires enter, exit, down, up, click or
  drag on a `shape3d`, `mesh`, `sprite` or `polygon`.
- **Key, action and resize hooks.** Polling only.
- **States.** No name for "this node's hover look"; a transition is a tween a
  script writes.
- **Variables.** No scene-level typed values with a change event; `settings`
  persists app preferences and `exports` are per node.
- **Bindings.** The Events view lists hooks; it authors nothing.
- **Rigs.** Orbit, first-person, third-person and click-to-move are each a
  script a project writes again.

## 1. Design

**Pointer events are picked headless and dispatched as hooks, on the tick.**
A system at the input stage resolves what the pointer is over — `pick_ray`
in 3D, a point query over `Renderable2d` bounds in 2D, widgets keeping their
own — and dispatches `on_pointer_enter(this)`, `on_pointer_exit(this)`,
`on_pointer_down(this, button)`, `on_pointer_up(this, button)`,
`on_pointer_click(this, button)` and `on_pointer_drag(this, dx, dy)` to the
nearest ancestor whose script declares the hook. Touch is a pointer. The
pick runs only when some script in the scene declares a pointer hook, which
the hook index the editor already builds knows, so a scene with none pays
nothing. Because input is recorded and picking is headless, the hovered node
is the same on replay: the hooks are deterministic by construction.

`on_key_down(this, key)`, `on_key_up`, `on_action(this, name, phase)` for
declared input actions, `on_scroll(this, dx, dy)` and `on_resize(this, w, h)`
follow, dispatched to every node that declares them.

**A state is a named set of property values; a transition is a tween across
all of them.** A `states` asset, inline or under `states/`, holds entries of
`<component>/<property>` paths — the vocabulary prefab overrides and clip
tracks already use — and a `states` component points at it:

```toml
[nodes.states]
library = "#door"
current = "closed"
transition = { duration = 0.3, ease = "out_cubic" }

[[assets]]
id = "door"
type = "states"
[assets.closed]
rotation_euler = [0, 0, 0]
"shape3d/color" = "#8a6a3a"
[assets.open]
rotation_euler = [0, 90, 0]
"shape3d/color" = "#f0c060"
```

`node.states.go("open")` and `node.states.go("open", #{ duration: 1.0 })`
tween every path through the sampler `balaur_anim` already has, so the
twelve easings are the same twelve; `current` is data, in the snapshot and
the digest, so a rollback puts a door back mid-swing. `on_state_changed(this,
name)` fires at the end. The base is whatever the node was authored as,
which is why a state lists only what differs.

**Variables are a typed table on the scene, in the digest.** `[variables]`
at the top of a scene file, spelled like `exports()` specs, with
`scene.variable(name)`, `scene.set_variable(name, value)`,
`on_variable_changed(this, name, value)` to every node that declares it, and
`persist = true` on one that should survive through `save`. They are what a
page sets through `docs/PLAN-embed.md` and what a binding's condition reads.

**A binding is one row: an event, a condition, an action, a target.**
`[[nodes.bindings]]` on any node:

```toml
[[nodes.bindings]]
event = "pointer_click"
when = "score >= 3"
action = "state"
target = "../Door"
value = "open"
```

The runtime is a small interpreter with one arm per action, each a call a
script can make; `when` is a Rune expression over variables, compiled once at
load. The action names are the surface below. The Events view authors these
rows, and "Convert to script" emits the equivalent `pub fn
on_pointer_click(this, button)` so a designer's scene graduates to code
without a rewrite. Bindings and scripts coexist on one node; bindings run
first.

**Rigs are presets over a camera or a character.** An orbit rig is `camera`
plus a shipped script module with `exports` for its limits;
`apply_preset("orbit_camera")` attaches both. `first_person`, `third_person`,
`click_to_move` and `platformer` do the same over `character3d` and
`character2d`, with the input actions the preset declares. They are ordinary
Rune under the runtime's own modules, so a project reads and forks them.

**Look-at and follow are modifiers.** `modifier3d` with `look_at` is
`docs/PLAN-animation-and-resources.md`'s; `follow` with `lag` and `offset`
joins it and `modifier2d`.

**A scene switch is one call.** `scene.switch(path, #{ fade: 0.3 })`
replaces the root with a fade the renderer draws; reset is a switch to the
same file. Spelled `switch`, not `load`, because N1 reserves `load` for
producing a live object from a path.

## 2. The surface

Spline's event and action lists, and where each lands.

| Spline event | Balaur |
| --- | --- |
| Mouse down, up, press, hover | `on_pointer_*`, step 1 |
| Key down, up, press | `on_key_*` and `on_action`, step 1 |
| Scroll | `on_scroll`, step 1 |
| Drag and drop | `on_pointer_drag`, with `on_pointer_drop(this, node)` when released over another node, step 1; OS files stay `input.dropped_files` |
| Look at, follow | modifiers, step 5 |
| Game controls | rigs, step 5 |
| Start | `init` (have) |
| Variable change | `on_variable_changed`, step 3 |
| Distance, trigger area | a sensor collider and `on_collision_start` (have); a `distance` binding condition, step 3 |
| State change | `on_state_changed`, step 2 |
| Collision | have |
| Screen resize | `on_resize`, step 1 |
| API updated | `on_response` (have) |

| Spline action | Balaur |
| --- | --- |
| Transition | the `state` binding action, step 2 |
| Animation | `animation.play`, `pause`, `stop`, `seek` (have) |
| Sound | `sound` play and stop (have) |
| Video | not planned until video playback exists (roadmap) |
| Open link | `engine.open_url` (have) |
| Reset scene | `scene.switch` to the same file, step 3 |
| Switch camera | `camera.current` (have) |
| Create, destroy object | `scene.spawn`, `scene.instantiate`, `node.free` (have) |
| Scene transition | `scene.switch` with a fade, step 3 |
| Particles control | `particles.emitting` (have) |
| Set variable, variable control | step 3 |
| Conditional | `when`, step 3 |
| Clear local storage | `save.remove` (have) |
| API request | `http.request` (have), with the response into a variable, step 3 |
| Code tab | a script (have) |

## 3. Steps

1. **Hooks.** The pick system, the pointer hooks, keys, actions, scroll,
   resize, the "any declared" gate. Ends with: a cube in `examples/hello`
   that lights on hover with a five-line script, and a replayed session that
   hovers it.
2. **States.** The asset, the component, `go`, the digest entry,
   `on_state_changed`.
3. **Variables and bindings.** The table, the calls, the interpreter,
   `when`, `scene.switch`. Ends with: the door above opening with no script
   in the project.
4. **The Events view authors rows** — `docs/PLAN-editor-ergonomics.md` step
   6, listed here because this plan is not done until a binding can be made
   without a text editor.
5. **Rigs and modifiers.** Five presets, `follow`.

## 4. What CI can prove, and what it cannot

- Hooks are headless: a recorded session with pointer movement dispatches
  the same hooks on every platform, and the digest says so.
- A state transition's end pose equals the state's table, and a rollback
  mid-transition resumes from the snapshot.
- A binding file round-trips through the TOML patcher with comments intact.
- Every Spline action in the table above has a test that runs it from a
  binding and from a script and gets one result.
- What it cannot: whether the Events view is usable by someone who has never
  seen Rune. That is a person watching a person.

## 5. Open questions

1. **Bubbling.** Nearest scripted ancestor is the rule above; whether a hook
   may return `false` to let a parent see it too is decided by the first
   scene that needs it.
2. **Cost of the pick.** One ray per tick is nothing; a point query over
   every 2D bound is linear in nodes and wants the broad phase a
   `Renderable2d` culling pass would want too.
3. **`when` as Rune.** A Rune expression is the honest choice — it is the
   language the scene already speaks — but it makes a binding depend on the
   compiler, which `docs/PLAN-embed.md` wants out of a shipped module. A
   tiny comparison grammar is the fallback.
4. **Where `[variables]` live for a prefab.** A prefab instance may want its
   own; that is `exports` on its root, and the rule may simply be that.
