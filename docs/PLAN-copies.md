# Plan: copies

A value is looked at through a reference. It is copied only where the copy
is the point. Handing a script a parsed 1.6 MB document took 1.9 s because
every map copied all its values to list them, at every level of nesting:
one `v.clone()` in a `map`. This plan is the rule, what holds it, and the
copies a review of the whole engine found (2026-11-21), to be taken down.

## The rule

1. **Borrow to read.** A function that only reads takes `&T`, `&str`,
   `&[T]`, an iterator of references. It does not take `T` and it does not
   clone what it was lent.
2. **Move what is done with.** A value its holder will not read again is
   moved (`into_iter`, `std::mem::take`, `mem::replace`, an `_owned`
   variant of a converter), not converted by reference and dropped.
3. **Share what does not change.** Data many hold and nobody writes (a
   parsed document, a schema, a table of names) is behind `Rc` or `Arc`; a
   holder that must end a `RefCell` or world borrow before something
   re-enters clones the `Rc`, not the data.
4. **A copy that stays says why.** Where the copy is the point, the line
   above it says so, starting `// copy:`. The reasons there are:
   - the value must outlive what it was read from (it crosses a thread, a
     frame, a call back into a script that may change the source);
   - a snapshot is wanted: the original is about to change and the old
     value is still needed (an undo, a put-back on refusal, a rollback);
   - a borrow must end before code runs that takes the same cell or the
     world, and the data is small or cannot be shared.
5. **A reference is not free either.** What is written through a shared
   value is seen by everyone holding it. So before a copy becomes a
   reference or an `Rc`: who else holds it, who writes it, and whether a
   reader was counting on the old value. Shared data is immutable data; a
   writer gets its own (`Rc::make_mut`, or the copy of rule 4).

Cheap handles are not copies in this sense: `Rc`, `Arc`, `SmolStr`,
`Entity`, an id, a `Copy` type. Cloning one is how it is shared.

## What holds it

- `scripts/copy_lints.py` (run by `scripts/precommit.sh --lints`): the
  shapes below, each a finding unless the line above says `// copy:`.
  - **copy-per-item**: `.clone()`, `.to_vec()`, `.to_owned()` or
    `.to_string()` inside the closure of a `map`, `for_each`, `filter_map`
    or `flat_map`, or `.cloned()` before a `collect` (the shape of the
    1.9 s: a copy an item, of what the items were lent from).
  - **copy-to-iterate**: `for x in y.clone()`, `for x in y.to_vec()`.
  - **copy-to-borrow**: `&x.clone()`, `&x.to_vec()`, `&x.to_owned()`.
  - **copy-out-of-a-cell**: `.borrow().<...>.get(k).cloned()` and
    `.borrow().clone()`.
  A fifth shape is reported and never fails, since only a reader can tell:
  **convert-by-reference**, a `from_*`/`to_*`/`dynamic` converter called on
  `&local`, which is a second copy where `local` is not read again.
  The lint cannot see a type, so a cheap handle cloned inside a closure is
  a finding too: `// copy: a handle`, or `.map(Rc::clone)`.
  It starts from a baseline, `scripts/copy_lints.baseline`, the findings of
  the day it was written: a finding not in the baseline fails, and so does
  a baseline line that no longer matches, so the file only shrinks.
- clippy's `needless_pass_by_value`, `implicit_clone`,
  `unnecessary_to_owned`, `assigning_clones` and
  `large_types_passed_by_value`, on with `pedantic`. `redundant_clone` is
  in `nursery`, which the workspace leaves off: step 1 turns it on by name
  once what it finds is cleared.
- Step 2's benches: a parse of a megabyte document handed to a script, and
  a screen of form controls filled twice, so the two costs this plan
  started from have a number a change is held against.

## Done

- `any_cell` hands out what a cell points at as borrowed parts (`Part`);
  an array's items are encoded into one buffer and written once.
- `json::parse` reads straight into script values (`Parsed`), no
  `serde_json::Value` between.
- An engine call's answer is moved into the script's value
  (`from_neutral_owned`, `dynamic_owned`).
- An awaited value is taken by the await that reads it, not cloned.
- A call into a module no longer copies its result kinds (`Rc<[Slot]>`).
- A block for what the engine hands a script is taken by the engine as
  the module's `__alloc` would take it, on the module's memory and its
  allocator's globals, with no call into the module (`runtime::heap`, on
  wasmtime and wasmi; the module still grows its memory). Same size class,
  same free list or bump, so the heap ends block for block as the module's
  own calls would leave it: `heap`'s tests hold the two to one sequence,
  and `memory`'s that what the engine hands over is freed, reused and the
  same on both engines. Modules now export `__bump` beside `__live`.

Measured on a quiet machine, debug engine, the 1.6 MB pack parsed for a
script: 60 ms before the host-side allocator, 29 ms after (200,000 ints in
a list: 68 ms to 37 ms). An earlier figure of 1,932 ms then 700 ms was
taken while the machine ran other builds at a load of about 300, which
inflated it tenfold: a time is taken on a quiet machine, or by the CPU
the process spent.

## The review

Three reviewers read the copy sites of every crate outside tests: about
3,560 of them (`.clone()`, `.to_vec()`, `.to_owned()`, `.to_string()`,
`.cloned()`). About 770 are cheap handles, about 2,000 are needed (an owned
answer to a script, a stored name, an error's text, a move across threads,
a snapshot that ends a borrow), and about **300 are avoidable**, listed
here by where they run. `[V]` was verified by reading the function and its
callers; `[I]` was inferred from the line. None is taken down until it is
read again and, where it is hot, measured. Paths are under `crates/`.

### Script boundary (`balaur_bs`, `balaur_script_bs`, `balaur_script`)

A value crossing is built two or three times: module memory, the runtime's
`Value` (L), the engine's `Value` (N). L is only ever between.

1. **Done.** `balaur_script_bs/src/lib.rs:63`, `convert.rs`: an answer
   converted by reference, then dropped.
2. `convert.rs:44-51, 61, 66, 86, 320-324` (`to_neutral(&args.value(i))`),
   callers `lib.rs:111, 117, 127, 145, 196`, `balaur_script/src/value.rs:
   300`: every str, list, `any` and record argument, already decoded from
   the module in `Slots::read` (`balaur_bs/src/runtime/args.rs:116-139`),
   cloned again into N. Fix: take trees out of `Slots` by index, or decode
   straight to N. Short strings keep the pooled buffer. The first copy is
   needed (a binding may re-enter the module and memory may move).
3. `balaur_bs/src/runtime/marshal.rs:47, 49, 51-54, 58`: a struct's field
   list and defaults `.to_vec()`, each field value cloned, once a level of
   nesting, for every `Vec2`, `Color` and record handed to a script. Fix:
   `Arc::clone(&self.meta)` and borrow; build a default only on a miss.
4. `runtime/values.rs:306-330` (`str_bytes`, `array_header`, `cell_bytes`):
   a `Vec<u8>` per string, per array header and per cell, to prepend eight
   bytes. Fix: the header from the stack, the body written from the slice.
5. **Done.** `runtime/host.rs:569, 618`: an awaited value cloned.
6. `balaur_script_bs/src/host/tasks.rs:96`: a wake's payload converted by
   reference; the last home to await it can be handed it. Two homes may
   await one token: the earlier ones still copy.
7. `balaur_script_bs/src/host.rs:988-993`: a broadcast's arguments
   converted N to L once an instance of the same script and method (the
   batch path at `:945` does it once). Fix: once a group. Encoding once in
   the module and retaining per handler would alias lists between
   handlers: safe only for `str`.
8. `host.rs:1018, 1043, 1096`, `host/tasks.rs:176`, `host/engine_facing.rs:
   362, 234`: `to_neutral(&v)` on an owned L just decoded. Fix:
   `into_neutral(v)`.
9. `balaur_bs/src/runtime/mod.rs:507, 527-528, 655`, `stepping.rs:68`: an
   object's class name `String` and its one-slot `Vec<Raw>` cloned for
   every call into a script, every batched instance a frame, every task
   poll. Fix: `Rc<str>` and `SmallVec<[Raw; 1]>`.
10. `runtime/values.rs:245-259` (`read_any`, a map): its keys read as a
    `Vec` of `String`, each turned into a `SmolStr`, then zipped into a
    third `Vec`. Fix: read each key straight into the pair.
11. `host/engine_facing.rs:41, 103, 113`, `runtime/mod.rs:389-393`: each
    scene property of a scripted node copied five times on its way into
    the module at attach.
12. **Done.** `runtime/wasmtime_engine.rs:992`.
13. `runtime/web_engine.rs:287-296`: in a browser every word read makes a
    `Vec` and two JS views, every write copies twice. Fix: `read_into`
    over `subarray().copy_to`, one kept view remade when memory grows.
14. `balaur_script_bs/src/lib.rs:171`, `balaur_script/src/value.rs:271`: a
    text property read and every `String` parameter copied to a host
    `String` only to be copied again. Needs bindings that read to take
    `&str`, valid only until the binding re-enters the script.
15. `runtime/host.rs:534-541` (`script_of`): two static names read out of
    the module as `String`s for every `node.script`.
16. `runtime/mod.rs:564, 667`, `stepping.rs:64`: `meta.function(key)
    .cloned()` (a `FnInfo` with a `Vec<Ty>`) for every call.
17. `runtime/mod.rs:943`, `balaur_script_bs/src/host.rs:144`: a `Ty` cloned
    for every get or set of a script's field.
18. `balaur_script_bs/src/lib.rs:112, 119`: two `Value::Str` and a
    one-entry map built for a component property get or set that misses
    the placed path.
19. `convert.rs:262` (`tagged`): an enum's fields cloned, then copied again
    by `record`.
20. `runtime/marshal.rs:133, 218, 228`: field lists `.to_vec()` in
    `default_of` and `release_flat`.
21. `host/links.rs:114`: two `String`s a link, for every link, on every
    attach. Fix: `Rc<str>`; the snapshot stays (a release can run a script
    that edits links).
22. `host/profile.rs:46`: `entry(path.to_string())` on a hit.
23. `balaur_script/src/value.rs:149, 161, 298`: N cloned an argument on the
    `&[Value]` path.
24. `runtime/mod.rs:170, 175, 245`, `host/units.rs:290`: a whole `Meta`
    and a whole `Compiled` cloned for each instance built and each reload.
25. Compile time, not a frame's: `host.rs:524` (`api()` returns the whole
    `HostApi` by clone, twice a compile), `balaur_bs/src/prelude.rs:118`
    and `codegen/mod.rs:584` (the prelude's whole tree, twice),
    `maps.rs:79`, `host.rs:487, 423`, `codegen/func.rs:181-220` (`for s in
    lv.slots.clone()`, six), `codegen/call.rs` (`sigs.get(..).cloned()`,
    about fourteen).

Beside the copies, on the same paths: a release plan rebuilt for every
str, array and `any` answer (`marshal.rs:169-202`); a `Vec<Raw>` an
argument and a call (`marshal.rs:14`, `engine.rs:40`); one call into the
module to count down each lent object (`mod.rs:495-510`); `Meta::fields`
and `is_class` linear over every struct (`lib.rs:425-433`), run a value.

**Measured, 2026-11-21, debug engine, wasmtime**, on a busy machine: an
engine call that answers a bool cost 1.4 µs, a value written into a
module 2.5 µs, where the module made 400,000 allocations of its own in
14 ms: the call into the module's `__alloc` for each value was what a
large answer paid. **Done**: `runtime::heap` takes the block from the
engine's side (see Done). Left on this path: the browser's engine and a
native build still call `__alloc` for each block (`web_engine.rs`,
`native_engine.rs`), the second copies above, and a release engine's
numbers.

### Core (`balaur_core` and the crates it carries)

1. `[V]` `file_api.rs:458` (`toml_encode`), `engine_api.rs:856-869`,
   `project.rs:703-713`: `scene::instantiate` takes text, so a script's
   table becomes a TOML tree, text, a parsed tree and a `SceneDoc`: four
   copies of every node, parsed again on every call (`project.rs:621`).
   Fix: `instantiate` takes a table; an instance by path and a table of
   overrides.
2. `[V]` `replay.rs:160` (`ExternalIo::drain` returns `recorded.clone()`):
   every arriving HTTP body, socket frame and platform event copied whole
   whether or not anything records. Fix: `mem::take` unless recording.
3. `[V]` `balaur_http/src/lib.rs:279, 294, 302`, `balaur_websocket/src/
   lib.rs:354`, `balaur_platform/src/lib.rs:399`: each event's whole value
   pushed into a snapshot nothing outside tests reads.
4. `[V]` `components/values.rs:392`, `assets.rs:690, 479, 698`,
   `batteries_api.rs:20`: an asset's whole definition cloned to check its
   keys, for every component write that names an asset: every node of an
   instantiate. Fix: `Rc<Definition>`, and the check kept by reference.
5. `[V]` `components.rs:782`: each patched value cloned into a table of its
   own, for every `patch_component` of more than one key and every prefab
   override. Fix: the shortcut `add` has at `:717`.
6. `[V]` `components/derived.rs:936`: the old value of every written
   property built, used only on a refusal. Fix: only where `after_write`
   can refuse. The put-back stays.
7. `[V]` `components/property.rs:261` (`holds_already`): an owned value
   built to compare. Fix: compare through `read_given`.
8. `[V]` `project.rs:842`, `scene.rs:126`, `node_api/scripts.rs:36, 47,
   153, 164`: a script's props, a deep list, copied for each scripted node
   of each instantiate and twice for each `attach_script`. Fix:
   `Rc<[(String, Value)]>`. No world borrow may be held across the attach.
9. `[V]` `events.rs:400, 624`: an event's payload cloned for each event
   delivered. Fix: move it into the arguments and take it back.
10. `[V]` `components/values.rs:225` (`overlay`): the whole table cloned on
    every `add` that names a colour, an asset or a record: most UI nodes.
11. `[V]` `components/values.rs:408`, `assets.rs:391`: an inline asset's
    table cloned on every write, dropped when its digest is known.
12. `[V]` `components/values.rs:38, 46`: each record value cloned twice.
13. `[V]` `engine_api.rs:915`, `strings.rs:396, 410, 421`: `strings::tr`
    clones its arguments, the looked-up text twice, then a `format!` and a
    `replace` an argument.
14. `[V]` `assets.rs:646-654`: every `[[assets]]` block cloned into a new
    scope, the same as the one it replaces, for each prefab instance.
15. `[V]` `file_api.rs:451` (`toml_parse`): a tree, then `from_toml(&tree)`
    cloning every string. The same at `node_api.rs:751, 760, 826`,
    `node_api/scripts.rs:65, 205`, `engine_api.rs:985`,
    `batteries_api.rs:21`, `settings_api.rs:80, 135`, `changes.rs:155`.
16. `[V]` `file_api.rs:687` (`json_encode`): a tree, then its text.
17. `[V]` `node_api.rs:515-525`: a name, a `format!` and a `json!` built
    for each child added before the replay log is asked whether it wants
    them. The same in http, websocket and platform.
18. `[V]` `variables.rs:216`, `states.rs:88`, `bindings.rs:484, 537`,
    `balaur/src/interact.rs:138, 166` (`with_source`): every argument
    cloned into a new `Vec` for each node that hears a broadcast.
19. `[V]` `states.rs:65`: a state's whole patch table cloned on each
    change. The copy ends a world borrow: `Rc`, not a held reference.
20. `[V]` `events.rs:431, 443`: every kept payload cloned, then filtered.
21. `[V]` `scene_api.rs:154`, `node_api/scripts.rs:140`: a clone only to
    pass `&local`.
22. `[V]` `changes.rs:200, 500`: a `Vec<SmolStr>` of keys for each
    component write, and every component's name a pump, dropped when
    nobody listens.
23. `facts.rs:126` (`platform()`): three `String`s to read one bool, once
    a UI pass (`balaur_ui/src/widget/arena.rs:251`).
24. `[V]` `balaur_input/src/actions.rs:483, 349`, `lib.rs:685`,
    `balaur/src/interact.rs:358`: every action's name, and the pads with
    their mappings, cloned a frame.
25. `[V]` `schedule.rs:174`: a `String` a system and a set, a stage, a
    frame.
26. `assets.rs:335`, `project_files.rs:328`: two `String`s for each
    `assets::load`.
27. `balaur_asset/src/texture_asset.rs:98, 112`: a path `String` for each
    sprite drawn.
28. `[V]` `scene/place.rs:278`: every placed node's `Entity` in a `Vec`
    for each reader a frame. Fix: `Rc<[Entity]>`.
29. `[V]` `components.rs:1064`, `project/instances.rs:59, 107, 257`,
    `file_api.rs:593` (`as_item`, a save's), `project_files.rs:235` (a
    packed file's bytes for each read), `dependencies.rs:576, 648`,
    `node_api.rs:440`, `settings.rs:199, 332`.

Recomputed on each call, where once would do:

1. `project.rs:621`: TOML parsed on every `scene::instantiate(text)`.
2. `project.rs:914` (`repair_ids`): two clones a node and two sets, for
   every prefab instance, of a document that did not change.
3. `project.rs:732-768`: for each prefab instance a path, every root, and
   a `stat` of the file.
4. `assets.rs:639`: a hash of the whole scene text for each build.
5. `project/instances.rs:69-98, 212-243`: every scene-key handler probes
   each node; then a linear scan a key.
6. `project.rs:665` (`children_first`): the whole subtree under the
   parent walked for each instantiate, so rows added one call at a time
   cost the square of their count. Fix: return early under two scripts.
7. `events.rs:214, 602, 618`: `format!("on_{name}")` for each queued
   event, a row's name before `Bindings` is checked.
8. `components/values.rs:293`: `Passes::of` and the defaults worked out
   though `Facts` holds both.
9. `components.rs:773, 929`: `schema.position` linear three times a key.
10. `file_api.rs:202` (`resolve`): every root cloned and canonicalized for
    each `fs::` call.
11. `regex_api.rs:15`: `Regex::new` on every `regex::` call.
12. `strings.rs:207, 373`: the content root followed two or three times a
    `tr`.
13. `events.rs:521`: every listener bucket scanned a frame.
14. `components/derived.rs:829` `[I]`: one archetype move a component a
    node at instantiate, where one insert a node would do.

### UI, text, render, animation, physics

Checked and found sound: the widget arena is kept between passes; no look
table is cloned a widget a frame (`Rc<Look>`, memoised by kind and role);
the text cache hashes a borrowed request and a hit allocates nothing; the
render mirror copies only dirty nodes.

Written controls (`balaur_ui/src/widget/write.rs`, where the pool's routing
and memo moved; the lines below are the pool's), for each control of each
fill whether or not it changed, about seventy allocations for a spec of
six keys:

1. `:163` (`merge`), `:130-143`: a map built from ten default keys, then
   every key and value of the spec cloned deep.
2. `:172`, `:372` (`as_value`): the whole table cloned again before it is
   known to have changed.
3. `:373`: the table last written cloned a third time.
4. `:498`: the nested controls cloned, each already cloned by `merge`.
5. `:469`: a fourth clone on the changed path.
6. `:477` (`hidden`): a map built and cloned for each spare slot, hidden
   already or not.
7. `:334, 397, 199-270`: a `String` key a control, the kind cloned three
   times.

Fix for all seven: compare the borrowed spec with what was written, inside
the memo's borrow; build owned data only for what differs; write after the
borrow ends (a write can re-enter the writer).

A widget read again (`balaur_ui/src/widget/`):

8. `arena.rs:117` by `:315`: the whole `Widget` (about 1.8 KB, some fifty
   `SmolStr`, nine `Vec`s) for each dirty widget a pass; and every pointer
   move, press and scroll frame marks one dirty (`input.rs:704`).
9. `arena.rs:117` by `:363`: the same for every widget when any node is
   added or freed anywhere. Fix for both: the heavy lists behind `Arc`,
   `Arc<Widget>` in the component and the arena, a dirty mark that says
   what kind of change it is.
10. `scroll.rs:61` (`rows_of`): a full `Widget` clone **every frame** for
    each scrolling row, column, grid and panel with no `axis`.
11. `schema.rs:131-153`: the whole `Widget` rebuilt on a write of two or
    more keys (core reports `Written::Whole`).
12. `beside_controls.rs:109`, `beside.rs:151, 278, 669`: a list's items
    copied component to widget on every write. Fix: `Arc<[Item]>`.

Every frame for each widget of its kind:

13. `rows.rs:92` (a `BTreeSet<String>` of what is picked, a list, tree,
    table), `rows.rs:196, 204` (egui temp data cloned out), `icons.rs:41`
    (a `String` an icon glyph), `kinds.rs:278` (a menu's options),
    `table.rs:258` (two lowercased `String`s a comparison, in a sort run a
    frame), `ease.rs:241-260`, `numbers.rs:358` (egui's `Visuals` for two
    colours), `theme.rs:437`, `button.rs:73, 120`, `measure.rs:695, 775`.

An event or a keystroke:

14. `input.rs:508` (the whole input snapshot every tick with an edit),
    `input.rs:91` (a stable id `String` an edit), `input.rs:555-612` (a
    field's text twice a keystroke), `input.rs:338, 359`,
    `typing.rs:113, 154, 158` (every unchanged line of a text area),
    `numbers.rs:199` (egui's whole `Style` for one font).

Render, animation, physics:

15. `balaur_render/src/text_component.rs:556`, `world_text.rs:237, 698`,
    `sync_2d.rs:513-540, 684`, `batch_2d.rs:65`, `batch_3d.rs:76, 198` (a
    key of two or three `String`s built to compare with the kept one, for
    each moving sprite), `kiss3d_backend.rs:907` `[I]`.
16. `balaur_animation/src/system.rs:488-525` (a component and a property
    name cloned into each patch, a track a step), `sampler.rs:299`,
    `machine.rs:663`.
17. `balaur_physics/src/follow.rs:151`, `dim2/follow.rs:134`,
    `vehicle.rs:155, 166`, `shared/character.rs:63`, `ragdoll.rs:147`
    `[I]`: a component cloned for each body a step.
18. `balaur_geometry/src/mesh.rs:916-927, 994`, `triangulate.rs:84`,
    `balaur_text/src/atlas.rs:287`, `glyph.rs:53, 94`: once an asset.

Recomputed every frame, where it could wait for its inputs:

1. `kinds.rs:859` (`stack`): a fresh measure of every child's whole
   subtree, each frame; also a fold's header and a window's title.
2. `input.rs:704`: every edit marks its widget dirty, so a hover, a press
   and each frame of a scroll re-read the widget, drop its looks, and
   re-sync and re-measure its whole subtree in taffy. Fix: kinds of dirty
   (paint, look, layout).
3. `input.rs:99` by `balaur_core/src/ids.rs:139`: the whole subtree
   collected and compared by string for each click and edit.
4. `taffy.rs:650, 906` (`gather`): a new map for every root and every
   scroll, tab, stack and fold solve, an insert a widget.
5. `layer.rs:626`, `ease.rs:135`: a style with a transition looked up five
   ways a call, three to six calls a widget a frame.
6. `icons.rs:19` by `balaur_text/src/lib.rs:981`: `shaper()` follows its
   files for each icon drawn.
7. `rows.rs:198, 771`, `table.rs:233`: the whole item list walked and
   sorted a frame, though only the rows in view draw.
8. `balaur_text/src/lib.rs:545`: the layout cache cleared whole at 4,096
   entries, so the next frame shapes everything again.
9. `layer.rs:152`: reachable widgets, focus stops and shortcuts (each
   chord parsed) worked out every pass, key or no key.
10. `arena.rs:497`, `taffy.rs:936`, `layer.rs:799`: a `Vec` over every
    widget a pass; every kept taffy node swept; a resource lookup a widget.
11. `layer.rs:789`, `text.rs:534`: a keyed caption resolved through
    `strings` for each keyed widget a frame.
12. `measure.rs:77-986` (`theme_at`): a climb to the root for every
    measured widget.
13. `balaur_text/src/lib.rs:890` `[I]`: a label's glyph mesh rebuilt each
    frame for unchanged text.
14. `theme.rs:518` (`load_fonts`): a font named late rebuilds every family.

## Steps

1. `scripts/copy_lints.py` and its baseline (429 findings on 2026-11-21),
   in `precommit.sh --lints`: **done**, a new copy of a listed shape fails.
   Left: `redundant_clone` on, its findings cleared.
2. The two benches, in `crates/balaur_bench/benches/engine.rs`. Done when
   both print a number, and step 3's and 4's changes are held against
   them.
3. Pooled controls (UI 1-7) and the widget re-read (UI 8-12, recomputed
   2): what every screen of an app pays on each fill and each scroll.
4. `scene::instantiate` by table, and an instance by path with overrides
   (core 1, recomputed 1-6): what every list an app builds pays a row.
5. The component write path (core 4-7, 10-12): what every node pays at
   instantiate.
6. The script boundary's second copies (2, 3, 8, 9, 10), the host-side
   allocator on the browser's engine and a native build (done on wasmtime
   and wasmi), and a release engine's numbers.
7. Arrivals (core 2, 3) and events (core 9, 18).
8. The rest, a crate at a time, the baseline shrinking to nothing.

Each step: read the site again, change it, run the crate's tests, and
where it was hot, the bench before and after. Each copy turned into a
reference answers rule 5 in its commit.
