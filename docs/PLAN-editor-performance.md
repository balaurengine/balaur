> **Status:** §3 step 1 built 2026-09-07: the outliner draws its viewport
> rather than the document, and §0 records what that was worth. The `table`
> kind now places its body the same way and the Profiler and Cost docks are
> on it, so §2 step 1 is built and still no crate has been added. §2 steps 3
> and 4 and §3 step 2, the inspector, are not built.
>
> Sections 5 to 7 ask a different question, on `examples/hello`: what the frame
> costs outside the docks. The instrument in §7 is built and the allocation
> fixes §5 names have landed; nothing else in this plan has.

# Plan: what the editor's frame costs

The editor draws one row per node of the edited document every frame, in Rune,
and pays for each row three times: once emitting it, once in egui laying it
out, once in the tessellator. The cost is linear in the size of the document,
so a 38-node project is free and a 2000-node one spends 26 ms on the docks
alone. This plan makes a list draw only the rows on screen.

## 0. What it costs today

Offscreen at 1600x1000 on an Apple M1, 1000 frames each. `--frames 200` is
**not enough**: a mean over 200 includes a startup frame of about a second and
reads four times too slow. 1000 and 5000 frames agree to within 3%.

```sh
balaur edit <project> --offscreen --frames 1000 --timings
balaur edit <project> --offscreen --frames 1000 --timings \
  --state "shut:rail,shut:left,shut:right,shut:bottom"
```

| project | docks | `ui` | `render cpu` | `scripts/update` | wall |
| --- | --- | ---: | ---: | ---: | ---: |
| `examples/angrynerds`, 38 nodes | open | 1.80 ms | 0.60 ms | 0.40 ms | 3.08 ms |
| `examples/angrynerds`, 38 nodes | shut | 0.52 ms | 0.53 ms | 0.42 ms | 1.62 ms |
| generated, 2000 nodes | open | **26.24 ms** | 7.43 ms | 3.13 ms | **39.54 ms** |
| generated, 2000 nodes | shut | 1.06 ms | 6.16 ms | 3.15 ms | 12.84 ms |

**Built 2026-09-07, the outliner on `ui.list`.** `egui::ScrollArea::show_rows`
places uniform rows by arithmetic and hands back the range on screen, so no
crate was added. `left.rn`'s recursive `walk` became `flatten`, which builds
the open tree as an array and draws none of it.

| 2000 nodes, `ui` pass | | wall |
| --- | ---: | ---: |
| before | 26.24 ms | 39.54 ms |
| drawing only the visible rows | 6.16 ms | 19.48 ms |
| the flattened tree cached | **2.8 to 3.9 ms** | **17.1 to 23.1 ms** |

Seven to nine times quicker at 2000 nodes. The cache is keyed on a `doc_rev`
bumped wherever `S.doc` is replaced, plus the two lengths, so a fold or an edit
rebuilds and a still frame does not. The docks now sit 1.7 to 2.9 ms over the
shut baseline of 1.06 ms, against 25.2 ms before.

Read the spread, not the middle: two runs of the same command differ by up to
70%, and the worst frame in a run reaches 500 ms. Something stalls
intermittently, and until that is found only the ratios here are safe to
quote.

Three things fall out of it.

The shell itself is cheap and flat. With every dock shut the `ui` pass is
about 1 ms whether the document holds 38 nodes or 2000, so the bars, the rail
and the viewport are not the problem.

The docks are linear in the document. They add 1.3 ms at 38 nodes and 25.2 ms
at 2000, which is **12.8 microseconds per node per frame**. At 2000 nodes the
docks alone are more than a 60 Hz frame.

The rest of the 2000-node cost is the scene, not the editor. With the docks
shut, 12.84 ms of wall is `render cpu` and `scripts/update` drawing and
stepping 2000 nodes, and that belongs to the renderer and the tick.

Two things the table does not say. An offscreen run takes a pass every frame,
while a windowed editor sets `ui.set_lazy` and skips passes when nothing
moves, so these are the numbers for dragging or typing rather than sitting
still. And shutting a dock hides its contents as well as its frame, so the
flat 1 ms is a floor for the chrome rather than a measurement of it.

## 1. Where the per-node cost goes

`inspector.rn` and `dock.rn` are 2,615 lines and run every pass. One inspector
row is five crossings before its control draws:

```rune
ui::horizontal(#{ height: 24 }, || {      // 1
    ui::spacing(6, 0);                     // 2
    ui::horizontal(#{ width: label_w(S) }, || {   // 3
        ui::label(label, ...);             // 4
    });
    ui::horizontal(#{ width: control_w(S) }, body);
});
ui::add_space(2);                          // 5
```

The outliner is the one that scales: it emits a row per node in the edited
document, so the 12.8 microseconds is that row, times every node, every frame.
Cutting the rows drawn cuts the Rune, the layout and the tessellation
together, which is why a virtual list is worth more here than a faster
interpreter.

## 2. A list that draws what is visible

`WIDGET_KINDS` has `GRID`, `FLOW` and `SCROLL`, and no table. Add one kind
that takes the whole model in one call and iterates it natively, so a dock of
N rows is one crossing rather than N.

1. **`table`**, built rather than taken: `egui_extras::TableBuilder` would
   have brought a crate for `show_rows` and a drag, both of which the kind
   now does itself, and `balaur_ui` ships in every exported game.
2. **`list`**, over `egui_virtual_list`, for rows whose height is not uniform:
   the outliner tree and the Assets grid.
3. The script API takes a row count and a callback, in the shape `ui.fold`
   already uses, so a dock hands over its model instead of looping. The
   callback runs for a visible row, so a list of 200 crosses about 20 times.
4. **`inspector`**, which crosses nothing per row. `inspector.rn` builds its
   rows from `model::schema`, and that is `scene::component_schema`, a Rust
   binding over the component registry. A kind taking a node and a component
   name reads the same registry and draws every row natively, so the script
   says which components to show and Rust draws them.

Step 4 is the shape to reach for wherever the model is already Rust. Step 1
and step 2 are for the lists whose model is not: a search result, a filtered
tree, anything a script computed.

Both crates track egui release for release and are on 0.36, the version the
tree pins; `egui_virtual_list` is one of the `hello_egui` set, all of which
are on 0.36. `egui_cable` and `egui_node_graph2` are on 0.31 and 0.29 and are
**not planned**; the graph canvas is `egui-snarl`, in
`docs/PLAN-authoring-without-code.md`.

None of this moves the editor into Rust. A script still says which docks
exist, which node is selected and what a click does, and it keeps hot
reloading. What moves is the inner loop, so a dock hands over a model instead
of emitting a row at a time.

This is the contents half. The chrome half is `docs/PLAN-editor-as-scene.md`,
which puts the bars, sheets, docks and tabs in a scene of `widget` nodes, the
way a game's menu already is in `examples/hello`. The two meet in the middle
and neither needs the other first. Open question 1 says which is worth more.

## 3. What moves onto it

In this order, measuring after each:

1. The outliner. It is the only list whose length is the document's, so it
   owns the 12.8 microseconds a node costs and it is the whole of §0's slope.
2. The inspector's property rows, which are bounded by a node's components
   rather than by the document, so they cost a constant.
3. The Assets dock, the Docs dock's function list, the Events view. The
   first two are on the kinds; the Events view draws a row of controls per
   binding, so it is a form rather than a list and stays hand-drawn.

## 4. What this costs to ship

`balaur_ui` is in the game runtime, not only the editor, and a web template is
prebuilt, so a crate the editor links is carried by every exported game. The
web module is 20.4 MB today. Measure each addition against
`docs/generated/features.md` before taking it, and split the template only when
`docs/PLAN-embed.md` splits it for everyone.

## 5. The viewport, which is neither contents nor chrome

Sections 1 to 4 are about docks. They are not the whole shell. The editor also
draws the transform gizmo and the scene overlays every frame, from Rune, in the
`update` stage rather than the UI pass. Neither is a dock, so a virtual list
does not touch either.

Measured on `examples/hello`, 1000 frames at §0's count, offscreen at
1600x1000, by stubbing one call at a time and re-running. Instruction counts
come from §7 and are a steady-state frame rather than a mean, so §0's warning
about short runs does not reach them.

| | instructions/frame | wall |
| --- | ---: | ---: |
| the whole editor shell | 71,436 | |
| `gizmo::draw` | 53,333 | 0.86 ms |
| `overlays.rn`, seven functions | 9,583 | |
| everything else, every dock included | ~8,500 | |
| the UI pass, for comparison | | 1.58 ms |
| **wall, frame to frame** | | **3.74 ms** |

Read both columns; they measure different things. An instruction count is the
same on every machine, which is what makes it a regression test. It also
under-weights a `ui.*` call, because one instruction there does a panel's worth
of Rust. So the gizmo is three quarters of the interpreter's work and a quarter
of the frame, while the docks are an eighth of the work and two fifths of it.

§0 measures a 20-node document, where the docks are still cheap. The two costs
scale differently and the split above holds only at that size. A dock is linear
in the document, at §0's 12.8 microseconds a node, so at 2000 nodes it is 26 ms
and everything here is a rounding error. The gizmo draws the selection alone,
so it stays at about 0.9 ms whatever the document holds. Fix the docks for a
large project; fix the gizmo for every project.

`gizmo::draw` rebuilds the manipulator from nothing each frame: twelve box
edges, the handles and the rings, one `render::draw_line` a segment, across 39
loops in `editor/scripts/gizmo.rn`. Nothing in it changes unless the selection,
the transform, the camera, the tool or the hovered part changes. Caching the
line list against those five inputs is the fix. `overlays.rn` has the same
shape and takes the same one.

These numbers were taken after the allocation fixes in `crates/balaur_ui`
landed, so they do not line up with §0, which was read from a binary built
before them and on a different project. The millisecond column is a 1000-frame
mean, at §0's count and for its reason; an earlier 150-frame read of the same
ablation put the frame at 4.64 ms and the gizmo at 0.83 ms, so the startup
frame moves the total and leaves the difference between two runs alone.

## 6. The tessellation a skipped pass still pays

`ui.set_lazy` skips the pass, not the drawing. kiss3d's `EguiRenderer::render`
clones every shape and tessellates all of them on every frame, whether or not a
pass replaced them. An idle editor therefore pays the tessellator in full, which
is most of what the web editor costs while nobody touches it. Caching the
primitives against the shapes that produced them makes an idle frame nearly
free.

This is not §1's `render cpu` line. That one is the tessellation a row
genuinely causes. This is the same work repeated on frames that drew nothing
new, and it is fixed in the fork rather than here.

## 6b. What a control costs to read

**Built 2026-09-20.** A pooled control read its whole `widget` component twice
a frame to learn the one property it carries, and a control that carries
nothing read it once for nothing. Each read built a TOML table of about forty
keys with the widget's class tables cloned into it, then converted that table
into script values.

`node.get_component(component, key)` now answers one property, and the `widget`
component answers the common keys straight off the struct through
`components::answers_property`. `pool.rn` asks for the one property it wants,
and asks for nothing at all where a control carries nothing.

Measured on `examples/hello`, offscreen, 600 frames, three pairs run back to
back on a machine that was also building: the `ui` pass reads 16.2, 18.9 and
16.9 ms against 20.4, 20.6 and 23.7 ms before, so about a fifth off. The
absolute numbers are inflated by the load; the ratio is what to read.

A sampled profile of the same run puts 63% of the main thread in
`balaur_ui::pass` and almost all of that in the Rune VM, so §2's kinds are
still where the rest is.

## 6d. What the registry costs every read and every write

**Built 2026-09-20.** §6b made the read of one property cheap for `widget`.
This is the half underneath it, which every component pays and no component
had to be taught.

Four things were wrong. `ComponentRegistry` was a `Vec` scanned by string, and
a single `patch` resolved the name four times: for the schema, for what was
asked before, to apply, and to record. `patch` then rebuilt the schema's
defaults into a fresh table on every call, wrote the component's own table over
them, and ran the colour pass and the asset pass over the result whether or not
the schema declared either. `has_component` built the whole table to answer a
bool.

The registry is now a `DetHashMap` keyed by name, so a lookup is a hash and
iteration is still registration order, which is what a component's bit in
`Attached` and the editor's section order both read. Registration works out
each schema's defaults, and whether it has any colour or any asset property,
once and holds them in `Facts`. A write resolves the name once and carries the
index. `patch` starts from the component's own table instead of from the
defaults, fills only the keys that table left out, and skips the colour and
asset passes a schema does not need. `has_component` reads the `Attached` bit.

Registering one name twice now panics naming it. Two definitions under one name
was silent before: every lookup answered with the first, so the second's
`apply` never ran.

Measured with `cargo bench -p balaur_bench --bench components`, release, Apple
M1, three-second samples.

| | before | after |
| --- | ---: | ---: |
| `node.transform.position = [..]` from a script | 2.42 µs | 1.74 µs |
| `components::patch`, one property | 1.41 µs | 1.17 µs |
| `components::get`, whole table | 344 ns | 283 ns |
| `node.get_component(name, key)` | 1.08 µs | 1.00 µs |
| `node.transform.position` read | 1.06 µs | 1.03 µs |

The write is what moved, and it is the one animation pays: a track drives one
property through `patch` every tick.

What is left, in the order the numbers rank it:

- **`patch` still reads the whole table.** `get` is a quarter of what a write
  costs and cannot be skipped: the component may have moved since, which is the
  reason `patch` consults it rather than trusting what was asked for.
- **`present_on` asks every definition.** It runs every component's `get` over
  one node to learn which are there. `Attached` would answer in one read, but
  `transform` rides in the node bundle and has no bit, so the bitmask cannot be
  trusted on its own yet. The same gap is why a presence test costs 370 ns on
  `transform` and 30 ns on a component the registry attached.

## 6e. A name is a number, not a small string

**Built 2026-09-21.** §6d left a script read of `node.transform.position`
spending 283 ns building the table and about 750 ns crossing the seam. This is
the seam half, and it went down a wrong path first.

The measurement was right and the reading of it was wrong. A binding call
taking nothing costs 190 ns, one taking an integer 216 ns, and one taking the
string `"transform"` 298 ns, so a string argument costs about 82 ns more than
an integer. A 54-byte string costs barely more than a 9-byte one, which says
the cost is an allocation rather than a copy.

The first answer was to stop allocating: `Value::Str` held a `SmolStr`, which
keeps up to 22 bytes inline. It bought nothing. The same three cases read 190,
216 and 298 ns afterwards, because the allocation is Rune building the string
value for the literal, on its own side of the call, where nothing the seam
does can reach it. It also made a string past 22 bytes worse, and it put two
string types in a codebase that wants one. It is gone.

**The answer every engine of this kind uses is an id.** Godot interns a
`StringName` and compares the pointer; Unreal's `FName` is an index into a
global table; Unity hands out an `int` from `StringToHash` and takes that
forever; Bevy addresses a component by a numeric `ComponentId`. A name is text
at the edges and a number at run time. Balaur already did this twice, for
`MaterialId::intern` and for the component registry's own index.

So the handle carries the number. `Component` holds the registry index beside
the interned name, resolved when the field was installed. `components` grew
`index_of`, `property_at`, `patch_at` and `node_api::set_property_at`, all
addressed by that index. A property's set of owning components is a `u128`
mask rather than a `HashSet<String>`, and the defaults a read falls back on
are a table indexed the same way. `PropertyReaders` is a `Vec` indexed by
component rather than a map keyed by name.

What that removes from one `node.transform.position`: two `String`
allocations, three hashes of the component name, and the trip through the
neutral value seam, which the backend no longer needs for a call whose two
names it resolved at startup.

## 6f. What all of it was worth

**Measured 2026-09-21**, release, Apple M1, five-second samples, load average
under six. Everything in §6d, §6e and this section together, against the
numbers §6d opened with.

| | before | after |
| --- | ---: | ---: |
| `node.transform.position = [..]` from a script | 2.42 µs | 1.28 µs |
| `node.transform.position` read from a script | 1.06 µs | 541 ns |
| `node.get_component(name, key)` | 1.08 µs | 805 ns |
| `node.has_component(name)` | 612 ns | 447 ns |
| `components::patch`, one property | 1.41 µs | 838 ns |
| `components::property`, one key | 372 ns | 100 ns |
| `components::present_on` | 947 ns | 149 ns |
| a presence test on the bundle's `transform` | 370 ns | 39 ns |
| a presence test on a registry-attached component | 37 ns | 43 ns |

The two presence tests now cost the same, which is the point of §6d's second
mask: there is no longer a component the bitmask cannot answer for.

One thing got slower. A name lookup is a hash at 19.5 ns where the scan it
replaced took 9.3 ns for `transform`, which registers first and was found on
the scan's first comparison. The scan was linear, so the last of the
forty-eight cost far more; the hash reads 20.4 ns there. A write pays one
lookup now instead of four, so it comes out ahead either way.

## 6c. The shell writes what changed

**Built 2026-09-20.** `pool.rn` patched every widget it drives on every pass:
the bars, the rail, the tabs, each inspector row and each of its controls. A
patch rebuilds the widget and dirties the layout, and a shell that nobody
touched asks for the same table it asked for last pass.

The pool now remembers what it last asked each control to hold and writes only
when the table differs. What was asked for is forgotten when a node is made,
when a host's children are rebuilt, and on the pass a reader's own edit landed,
since the value on the node is then not the one in the table.

Measured on 300 frames, offscreen at 1920x1080, interleaved pairs, min of
three:

| | before | after |
| --- | ---: | ---: |
| `examples/hello`, docks open | 11.33 ms | 6.04 ms |
| `examples/hello`, every dock shut | 5.18 ms | 2.88 ms |
| `examples/angrynerds` | 10.85 ms | 6.14 ms |

Where the rest of it sits, on `examples/hello` before this landed: the chrome
with every dock shut was 5.8 ms of the 12.3, the inspector 2.7, the Output
dock 2.4 and the outliner 1.5.

## 7. The instrument

`engine.profile_scripts(on)` and `engine.script_costs()` count VM instructions
per script, and the Profiler dock has a `scripts` toggle that turns them on.
Instructions rather than milliseconds, so two runs of the same frame report the
same number and a change in the reading is a change in what a script does.

`--timings` also names each GPU pass, which the fork already timed and the
backend only totalled. On `examples/hello` at 1920x1080 the editor's 6.9 ms of
GPU is tonemap 2.7, opaque 2.0, the 2D pass 1.9 and shadows 0.3, and at a
quarter of the pixels it is 2.6 ms, so the frame's floor is fill rather than
anything the scene holds.

The editor compiles as one Rune unit, so the count is the whole shell rather
than a figure per file. Ablation is what localises it, as §5 did: stub one call,
re-run, take the difference.

## Phases

1. The `table` kind and the inspector on it. Built without a crate: the kind
   places its body by `ScrollArea::show_rows`, the way `list` and `tree`
   already did, and its columns are shares of the width a drag on the header
   moves. The inspector is still rows from `model::schema`.
2. The `list` kind and the outliner, against a scene with 50,000 nodes.
3. The remaining docks, and a self-test state that fails when the shell costs
   more than a frame. The Profiler, the Cost dock and the Library's templates
   are on `table`, and the outliner's search view draws on the same `tree`
   node the unfiltered walk does, so no view of the document emits a row at a
   time any more. What still does draws controls per row rather than rows.
   `rowsdemo` is the self-test state that reads the outliner's node back.
4. The gizmo and overlay line lists cached against their inputs, and the
   tessellation cache in the fork. Neither waits on the kinds above.

## Open questions

1. **How much is the docking chrome.** Shutting a dock removes its contents
   too. A run with every dock open on an empty panel splits the two, and
   decides whether `egui_dock` or `egui_tiles` is worth measuring at all.
2. **Whether the interpreter is the floor.** A sampled profile of the UI pass
   reads 36% Rune VM, 23% allocator, 10% egui and 3% tessellation, so the
   interpreter and the garbage it makes are most of it. If the docks still cost
   a frame once the rows are virtual, the next lever is the Rune call itself,
   and that belongs in `docs/PLAN-scripting.md`.
3. **What the budget is.** A 60 Hz frame is 16.7 ms, and the editor shares it
   with the game it is editing while playing.
