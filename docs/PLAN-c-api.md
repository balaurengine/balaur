# A C API for addons

Status: **Tier 1 shipped.** `crates/balaur_plugin/src/capi.rs` is the
implementation, `crates/balaur_plugin/include/balaur_extension.h` the committed
header, and `examples/extension_c_counter/counter.c` an extension in C that a
script calls. The tiers above it are open, and this is what they would be.

## What Tier 1 does not do

Worth stating plainly, because the gap is what a real addon will hit first:

- No components, systems, scene keys, or ECS access — that is Tier 2, and it
  still deserves to wait for an addon that asks for it.
- No calling *back* into script. `Value::Callback` crosses the boundary as an
  id, but there is no `api->invoke` yet, so a C extension cannot run a script
  function it was handed.
- No `Engine` access at all: a C function receives its own `void*` and its
  arguments, and nothing else.
- No asynchrony. A Tier 1 extension has no `ExternalIo`, no await token and no
  way to land an answer on a tick, which is why a platform's own services are
  engine crates behind features rather than extensions (`docs/PLAN-apple.md`).

## The tiers above

**Tier 2 — components and systems.** Registering a component type, a per-frame
system, and a scene key. Now an addon is a real plugin.

- Cost: entity ids and component storage cross the ABI, plus a query
  interface. Component data would have to be described (a schema) or passed as
  `Value`, and the cheap version of that is slow, while the fast version pins
  memory layout into the ABI.
- This is the tier where the ABI stops being obviously stable, which is why it
  waits for a real addon to ask: the component and query ABI is expensive to
  change once published, and guessing at it now means guessing at what addon
  authors need before any exist.

**Tier 3 — the whole `App`/`Engine` surface.** Everything a Rust plugin can
do. Not recommended: it freezes internals that are still moving, and every
future refactor becomes a compatibility problem.

Loading addons the other way round — the engine `dlopen`ing an addon rather
than an addon linking the engine — is the mirror image and needs its own
decision. The tiers describe the API either way.

## Open question

Is the goal addons *for the editor* (tools, importers, panels) or addons *for
games* (native gameplay code)? The answer changes what Tier 2 should expose
first — the editor is itself a Balaur project, so editor addons may be better
served by scripts than by C.
