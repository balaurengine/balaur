# Balaur Script: each module compiled once

Until 2026-10-23 a `.bs` script was one wasm module with every file it
`use`s linked in, transitively (`crates/balaur_bs/src/modules.rs`), and
wasmtime compiled that module to machine code. A file used by every script
was compiled into every script. A Godot project converted by the importer
of the time used its shim `gd.bs` and `classes.bs`, and `classes.bs` used
every object class, so each of the 364 scripts of `../polyglot-pirates-game`
linked about 715 files and 8 MB of source: the game never finished booting.

## What was chosen

**Units.** Each file is compiled alone, once, as a *unit*
(`Options::unit`). A script is its own unit linked, when it loads, with the
units of every module it uses, near or far.

- **Declarations, not bodies.** A unit reads the modules it uses for what
  they declare: structs, constants and what each function takes and gives,
  bodies dropped. Its direct `use`s are read whole; a module only their
  declarations name a type of gives its structs, their methods and its
  constants, all a file that does not `use` it can reach; a module its
  modules use for their own bodies is that module's unit's business.
  `Parsed` keeps each file's declarations and where each `use` led across
  compiles, until the host says a file changed, with a hash of what each
  declares that leaves positions out.
- **Links.** A call into another unit goes through a stub: it calls through
  a slot of the script's `fn` table, at the unit's link base plus the
  function's link number. Only the module functions a unit's own code names
  get one; the rest get no index. An async function is three links, `$start`,
  `$poll` and `$frame`, the last answering the frame size a caller in
  another unit allocates (the static build patches it in as a constant).
  A unit exports each function another may call as `~key`.
- **One memory.** Every unit of a script imports the script's memory, its
  `fn` table and the allocator's and debugger's six globals, so strings,
  lists, `any` values and class objects pass between units by pointer and
  one allocator frees them. Its static strings sit at its memory base, its
  `fn` values at its table base: both imported, laid out by the runtime.
  Each unit carries its own copy of the eleven runtime helpers, which share
  those globals.
- **Made on the first call.** `Runtime::new` instantiates the script's own
  unit. A link slot to a unit not made yet holds a stand-in, one per
  function and shared by every slot calling it, that makes the unit
  (machine code from the cache, data copied into its reserved place); every
  slot waiting for that unit is filled then. A unit no call reaches costs a
  reserved, untouched stretch of memory.
- **Compiled once.** The host keeps each file's unit until an edit
  (`BsHost::unit`), and on disk beside the machine code, as
  `Compiled::to_bytes` writes it: named by the engine's API, the file and
  its source, headed by each module it read and the hash of what that
  declared, and written by a compiler whose own source hashes the same. A
  warm start compiles nothing. wasmtime and wasmi keep each unit's module by
  hash for every script and store (`unit_module`), and wasmtime's disk cache
  keeps its machine code across runs as before.
- **An edit** rebuilds the file's unit, and every unit built against it when
  what it declares changed; every loaded script whose units include it
  reloads, objects carried across by field name as before.
- **Every build a host runs is units.** A unit is built with a debugger's
  probes, a profiler's counters and a browser's metering and direct imports
  (below), and an export's native build holds plain ones. A compile without
  `Options::unit` still makes one module with every file it uses linked in:
  the compiler's own suite runs those, and `analysis` checks a buffer with
  one. No host runs one, and `BALAUR_BS_UNITS` is gone.

Host calls are dispatched per unit (`HostState::bind_unit`), by its index in
the instance, each with its own types: a unit names a module struct
`gd::Thing` where the module calls it `Thing`. An engine call from a module
that reenters the script finds the script's methods in its own unit.

## Probes, counters and metering in a unit

Built 2026-11-03. What each build adds is the unit's own business, so a unit
with probes links with one without: a stub and a `~key` export are the same in
both, and a typed call runs from one into the other.

- **Shared and own globals.** The fuel, the statement count and the callees'
  share of the call running (`words::COUNT_GLOBALS`) are globals of the
  instance like the allocator's, imported by the units built to count. A
  debug unit's `__dbg_unit` and `__dbg_breaks`, a profiled unit's `__prof` and
  a direct unit's `__host_ctx` are its own; the engine answers each in
  `engine::own_global`. The native build's C imports none of these, and reads
  as it did.
- **A unit's tables are in its static data.** A debug build's breakpoint table
  and a profiled build's rows are zeros past its strings (`Unit::breaks_at`,
  `Unit::rows_at`), part of the block it is given when it links.
- **One shadow stack.** Every instance reserves its 64 KB past the units known
  when it is made, untouched without a debug unit. A shadow frame names its
  unit, so a stack across units reads each frame by that unit's probes and
  types and says each frame's file (`Frame::file`). A unit without probes
  pushes no frame.
- **A stop holds a call, not the instance.** The instance moves to wasmi once a
  debugger asks about a file of it, and only that file gets probes. Every call
  in it then runs resumable, so a typed call from a unit with no probes can
  stop in one that has them, and every unit is made first, since a stand-in's
  call cannot park. While a call is parked others run above its frames on the
  shadow stack and pass any stop by: one stop at a time. A trap among them goes
  back to the stop's stack. Rebuilding the instance waits until the stop goes
  on, as its memory holds the parked call.
- **A browser's engine has no stand-ins.** A slot of a page's table takes only
  a module's own export, so nothing can hold a slot for a unit not made yet:
  what is linked is made together before the next call, then the slots are
  filled. A module is kept by hash, so remaking an instance compiles nothing.
  A direct unit reaches the engine's entry points with its own id, one per
  unit, which is how a call finds the types it was made with.
- **Each build is kept apart.** `BsHost::unit` holds a file's unit per build
  (`host/units.rs`, `Build`), on disk too, so the second profile of a run
  compiles nothing and turning one off rebuilds nothing.

Decided: on a profiler or a debugger a whole instance is made again rather than
one file swapped into it, since a slot once filled has no way back to a unit's
old code and an object's unit is its index. What it costs is carrying the
instance's objects, which an edit already does.

## One instance per project

Planned and built 2026-10-27 (PLAN-balaur-script §9 step 4). A unit was
compiled once but made once per script: a module 40 scripts use was made 40
times, in 40 memories, and one script reached another only through
`node.call`, its arguments deep-copied. The units stay as they are; what
changed is how many times they are made.

- **One memory and one table for the project.** The host makes one instance
  whose own unit has no code (`BsHost::project`), and every file built as
  units is linked into it as it loads (`Runtime::link`), so `util.bs` is
  made once and a class object passes between scripts by pointer. A unit
  linked after the instance was made takes its static data from the heap,
  since the space before the heap was laid out for the units known then.
  An object and a task know their unit, and a call on one goes there
  (`Runtime::in_unit`).
- **A call to another script is a link**, as a call into a module is, typed
  by a checked cast: `let tree: scene_tree::SceneTree? = node.script;` is
  the node's script object when it runs that file's class in this instance,
  else nil (`__script_of`, answered by the host from the node's instance).
  The compiler knows each class's file from the linker.
- **An edit** rebuilds the file's unit and the units built against it, and
  the project's instance is made again with every file linked, each object
  carried across by field name. A field that held another carried object
  holds its replacement, so a script two others hold stays one. A reload a
  script asks for while the instance runs is built at once, so it says what
  broke, and swapped in between frames.
- **A second instance beside it.** A file from outside the project, as the
  editor reads a game's scripts, joins an instance of its own kind
  (`Place::Aside` in the host), never native. Debugging or profiling a game
  makes only that one again, and a stop in it holds none of the editor's
  scripts. A file a native build lacks waits there until the project has moved
  off native code.
- **`node.call` stays between the editor and a plugin**, and between the
  editor and the game it plays. Inside one instance a script calls another by
  its type: the editor's panels call the Editor so since 2026-11-06.
- **Not yet**: two scripts holding each other is a cycle the counts never
  free (a decode reads the inner one as nil rather than loop), and a
  carried object's deeper references, in a list or a struct, are copied.

## Weighed and not taken

- **One module for the whole game.** Simple, and every module compiled
  once, but every edit rebuilds the game and one broken file stops all.
  One instance (above) keeps each file its own unit.
- **wasmi's lazy translation for very large modules.** Cheaper machine
  code, but the front end still read 8 MB of source for each script.
- **Dead-code elimination in the linker.** `classes.bs`'s dispatch reaches
  every object class, so a converted project keeps nearly everything.

## Measured

Release builds on the M-series Mac this was written on, 2026-10-23, the
same binary both ways (a switch since removed built the whole file). The game
is `../polyglot-pirates-game` as `balaur import` converted it then; the
importer has since been removed.

| | whole file per script | units |
|---|---|---|
| Converted game, first boot, 3 frames | killed at 15 min, 4.1 GB resident | 91 s, 0.5 GB |
| Same, second boot | 324 s, 4.5 GB | 89 s, 0.4 GB |
| Its machine code on disk | 5.5 GB (30 MB a script) | 35 MB |
| `balaur check` on it | 520 s | 6 s |
| Editor boot, warm, instructions retired | 7.97 G | 5.97 G |

The game's boot left is its own scripts running, not compiling: host calls
that reach a module export by name. The editor's warm boot reads every unit
from disk (before the units were kept there it cost 1.6x the whole file's,
each of its 62 units compiled again). Wall clock on that machine is noise
under the load of the other builds it ran; instructions retired are not.

Measured 2026-11-03, a dev build of the tree before these builds linked units
against one after, on that Mac under the load of other builds: `balaur edit
--offscreen --state suite:test:profiler`, the profiler switched on and off.
Instructions retired are the run's less a boot of 12 frames (48 G with nothing
kept, 3.7 G warm, the same for both binaries), and the time is the state's.

| | whole file per script | units |
|---|---|---|
| The editor as it is, nothing kept | 34 G, 3.7 s | 29 G, 4.7 s |
| The same, kept from a run before | 4.0 G, 1.5 s | 7.3 G, 1.0 s |

A throwaway copy of the editor whose 39 panels hold `editor::Editor?`, what
compiled the editor once per panel, ran `test:profiler` and `test:theme_switch`.
Instructions retired are the whole run's, boot included.

| | whole file per script | units |
|---|---|---|
| Nothing kept | 1,135 G, 287 s, the link nil | 96 G, 10 s |
| Kept from a run before | 125 G, 137 s, the link nil | 10 G, 1.7 s |

Measured 2026-11-06, the panels calling the Editor by type against the same
tree with them calling it by name, one dev build, two runs each that agreed to
1%: instructions retired, and the state's time on that Mac with the machine
quiet. A boot is 12 frames; a state's cost is its run less a boot.

| | by name | by type |
|---|---|---|
| Boot, nothing kept | 52.1 G | 63.9 G |
| Boot, kept | 3.17 G | 3.44 G |
| `test:profiler`, nothing kept | 39.9 G, 1.7 s | 52.9 G, 2.0 s |
| `test:profiler`, kept | 1.45 G, 0.19 s | 1.89 G, 0.23 s |
| `test:theme_switch`, kept | 0.86 G, 0.09 s | 0.87 G, 0.08 s |

What a panel's unit costs more is reading the Editor's declarations, and the
modules they name, as it compiles: once a run when nothing is kept. Before a
carried link was left unread, the profiler's rebuild read the Editor once for
each panel holding it, 1.5 G more kept. `scripts/edit_cost.sh`, M
instructions an edit by name and by type: text 3.0 and 2.9, colour 3.0 and
3.1, position 1.0 and 1.2, engine text 2.5 and 2.6, one run each.

