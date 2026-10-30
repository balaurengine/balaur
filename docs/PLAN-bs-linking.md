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
- **What still builds the whole file.** A debugger's probes, a profiler's
  counters, a browser's metering and direct imports, a native build an
  export made, and `BALAUR_BS_UNITS=0`. An export compiles the whole file,
  as its C translation and the browser need.

Host calls are dispatched per unit (`HostState::bind_unit`), each with its
own types: a unit names a module struct `gd::Thing` where the module calls
it `Thing`. An engine call from a module that reenters the script finds the
script's methods in its own unit.

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
- **A file built whole** for a debugger's probes, a profiler's counters, a
  browser or a native export is an instance of its own, as before; turning
  the debugger on moves its objects out of the project's instance, and off
  moves them back. `node.script` there finds only that file's nodes.
- **`node.call` stays between instances**: the editor and a plugin, the
  editor and the game it plays. Inside one instance a call by a method's
  name in a string is the step after (PLAN-balaur-script §9 step 5).
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
same binary both ways (`BALAUR_BS_UNITS=0` builds the whole file). The game
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
