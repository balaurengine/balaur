> **Status:** in progress, 2026-09-08. Written from the inspector question
> "why is Transform hand-written when every other section is generated". Step 3
> depends on component property fields, which are in the working tree
> uncommitted (`crates/balaur_script_rune/src/value/component.rs`).

# Plan: transform as a component

## 0. Where the tree is today

- `Transform { position: Vec3, rotation: Quat, scale: Vec3 }` is a plain ECS
  struct (`balaur_core::scene:97`), not in the `ComponentRegistry`. There is
  one of them, not a 2D and a 3D one: a 2D scene uses the same struct with `z`
  as depth. The dimension split elsewhere — `body2d`/`body3d`,
  `shape2d`/`shape3d` — exists because rapier2d and rapier3d hold different
  data. A transform does not.
- Its scene keys are `position`, `rotation_euler` and `scale` at the node's
  top level, not under a `[nodes.<name>]` table, and the file stores rotation
  in radians.
- Its script API is methods on the node: `position()`,
  `set_position(x, y, z)`, `rotation_euler()`, `rotation_degrees()`, `scale()`
  (`balaur_core::node_api:47`).
- The inspector's Transform rows are hand-written (`inspector.rn:184`) — the
  only section in the panel that is properties rather than commands and is not
  generated from a schema. They convert radians to degrees by hand.
- `node_bundle!` gives every node a `Transform`, a `GlobalTransform`, an
  `Appearance`, a `GlobalAppearance`, `Children` and a `NameIndex` in one
  tuple, because inserting a component after the spawn moves the entity to
  another archetype, and that move was the hottest function of a script adding
  children (`scene.rs:269`).
- Nothing requires a transform to be present. `propagate_transforms` hands the
  parent's global straight down when a node has none (`scene.rs:532`),
  `with_transform` answers "node is dead or has no transform", and all 35
  reads across the crates are already `if let Ok`.
- Component handles carry a field per schema property:
  `node.collider3d.density = 15` reads through `get_component` and writes
  through `patch_component`, dispatched by component name.

## 1. Design

**The transform is a component like every other one, with no exemption.** Its
properties live in a `[nodes.transform]` table beside `[nodes.shape3d]` and
`[nodes.body2d]`, reached by the scene key handler `register_component` already
installs. A node's own keys are the ones that describe the node rather than
its data: `id`, `name`, `parent`, `tags`, `script`, `instance` and `overrides`.
Nothing else sits there.

This breaks every scene file, which `docs/NAMING.md` classes as the breaking
tier, and the break is the point: a transform read from `position` at the node
and written back to `[nodes.transform]` would be two spellings of one value,
which is the inconsistency this plan exists to remove. Every `.toml` under
`examples/`, `editor/` and the test fixtures moves in one pass.

**A schema property may name the unit it is shown in.**
`unit = "degrees"` joins the spec vocabulary in `balaur_core::components`, and
`validate_property` holds it to a closed set alongside the closed type list.
It is display metadata and nothing else: the file stores radians, the script
reads radians, and only the inspector row converts. `step`, `min` and `max`
are declared in the display unit, so `rotation_euler` reads
`{ type = "vec3", unit = "degrees", step = 0.5 }` and drags half a degree a
pixel rather than half a radian.

**A node need not have a transform.** The engine already behaves as if this
were true, so the change is to stop forcing it: no `[nodes.transform]` means no
`Transform`, exactly as no `[nodes.shape3d]` means no shape. A scene of five
thousand widgets writes none and carries none.

The archetype move the node bundle was built to avoid is paid by nobody.
`node_bundle!` keeps `GlobalTransform`, so a bare node still has a world
position and every renderer and physics reader is untouched; the local
`Transform` moves to a second arm of the same macro, chosen in the one spawn.
`scene::spawn_node` takes that arm, so a node a script creates has a transform
as it does today; the loader takes the bare arm when the file names no
`[nodes.transform]`.

**`node.transform.position` is how a script says it.** The property fields on
component handles give this for free, reading and writing one property without
building a table. It becomes the preferred spelling everywhere — the manual,
the generated reference, and every example is rewritten to
`node.transform.position` and `node.transform.position = v`.
`node.position()` and `node.set_position(x, y, z)` stay: they are in every
published script, they cost one binding each, and removing them buys nothing.

**Appearance follows in the same break.** `Appearance` (`visible`, `z_index`,
and whether `z_index` adds to the parent's) is the same shape as `Transform`:
a struct in the bundle, edited by hand, with keys at the node. Leaving it
there would keep exactly the inconsistency this removes, and moving it later
would break every scene file a second time. It is done after the transform
works, before the scene files are written once.

## 2. Steps

1. **The `transform` component.** Registered in `balaur_core` with a schema of
   `position`, `rotation_euler` and `scale`. `apply` and `get` read and write
   the ECS struct that is already there, so `propagate_transforms` and all 35
   readers are untouched. `SceneNode` loses its three typed fields and
   `NODE_KEYS` its three entries; a `[nodes.transform]` table reaches the
   handler `register_component` installs, and an override patches it as it
   patches any component.
2. **The bare node.** `node_bundle!` gains an arm without `Transform`,
   `scene::spawn_node` keeps the arm with it, and the loader picks by whether
   the file names the component. `remove` takes a transform away from a node
   that has one.
3. **`unit` in the schema.** The key, its closed set, `validate_property`, and
   `edit_float`/`edit_vector` converting for display and back on write. The
   generated reference states the unit on the property.
4. **The inspector's Transform section goes.** `transform_section`,
   `vec3_row` and their history keys are deleted; the generated section draws
   the same three rows in the same order, because `keys_sorted` is alphabetical
   and `position`, `rotation_euler`, `scale` already sort that way. The persona
   filters gain `"transform"` where `transform_section` was called.
5. **Every scene file.** `position`, `rotation_euler` and `scale` move under
   `[nodes.transform]` in every `.toml` under `examples/`, `editor/` and the
   crates' fixtures, in one mechanical pass.
6. **Examples and docs.** Every `.rn` under `examples/` moves to
   `node.transform.position` and `node.transform.position = v`; the scripting
   manual page and `docs/EDITOR-SCREENS.md`'s inspector sketch follow.

## 3. What this does not do

- It does not split the transform into 2D and 3D. There is one struct, and a
  2D scene reads `z` as depth. A 2D inspector that wants two axes and one
  angle is a display question the personas already answer, in the same place
  `unit` is read.
- It does not remove `node.position()`. The method form stays as the short
  one.
- It does not move `Appearance`, `Children` or `NameIndex` out of the bundle.
- It buys no measurable frame time. A node without a transform saves 80 bytes
  and one quaternion multiply in a pass that visits it anyway to push the
  global appearance down. The reason to allow it is that a component behaves
  like a component; the reason to expect a win is not there.
