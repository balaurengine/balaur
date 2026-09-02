> **Status:** phases 1–3 built on 2026-09-02 — the `debugger` script module,
> the engine freeze, Luau breakpoints with continue and step over / into /
> out, call frames with named locals, and the editor's gutter, dock and
> shortcuts. Phases 4–6 (Rune, break-on-error, a Debug Adapter Protocol
> server) are not started. See [generated/](generated/) for what the code
> actually does.
>
> **Where the implementation decided differently:**
>
> 1. **Frames and locals are captured inside the hook**, not after the
>    break. A parked frame's saved pc still points at the instruction it
>    stopped on, so `lua_getinfo` and `lua_getlocal` asked afterwards answer
>    for the instruction before it — the line is off by one and a local
>    declared on the previous line is missing. Inside `debugbreak` and
>    `debugstep` Luau has adjusted the pc for exactly this.
> 2. **O2 stays, and inlining is a documented limit.** The Luau compiler
>    inlines plain local functions at optimization level 2, so step-into
>    steps over one and a breakpoint inside it never fires; methods on the
>    class table are not inlined. Dropping to O1 in dev would trade that for
>    bytecode that differs from what ships.
> 3. **The module is `debugger`**, because `debug` is Luau's own library
>    and the host folds a module of that name into it.

# Plan: a script debugger

Breakpoints, stepping and a call stack with locals, for scripts running in
the editor, without forking either language.

The one-line finding: **both VMs already expose the primitives, and the shape
of the debugger is decided by Balaur, not by them.** Play-in-editor runs the
game in the same process and the same Luau state as the editor UI, so a
breakpoint can never block. It parks the script, returns to the frame loop,
and the editor draws the debugger on the next frame.

## 0. Where the tree is today

| | Status |
|---|---|
| Debugger | **None.** `crates/balaur_core/src/replay.rs` is a determinism debugger (record, replay, `--verify`); there is no breakpoint, step or variable view for a script. `docs/PLAN-implementation.md` recorded the gap for Rune and budgeted nothing. |
| Luau hooks | **In the VM, unwrapped.** `lua_breakpoint` patches an opcode on the script's proto and recurses into nested functions. `debugbreak` and `debugstep` callbacks, `lua_break`, `lua_singlestep`, `lua_getinfo`, `lua_getlocal`, `lua_stackdepth` are all declared by `mlua-sys 0.12`; `mlua` wraps none of them and treats the break status as an error. |
| Luau pausing | A coroutine that hits `lua_break` returns from `lua_resume` with status `LUA_BREAK` and its stack intact; resuming it continues. `init` and handlers already run as coroutines (`run_task`); `update` runs through a plain call. |
| Luau debug info | Bytecode is compiled at debug level 1: line info and function names, no local names. Level 2 keeps local and upvalue names and changes no semantics. |
| Rune | `VmExecution::step` runs one instruction; `Vm::ip`, `call_frames`, `Unit::debug_info` map an instruction to a source span. Locals are unnamed slots. |
| Editor | `ui.code_editor` paints a line-number gutter and nothing in it. The transport pause stops physics only; scripts keep ticking. The editor boots a Luau-only host, so a Rune game cannot play in it. |
| Frame loop | One accumulator drives `Stage::FixedUpdate`; nothing can hold the simulation still while the editor keeps drawing. |

## 1. Design

### Pausing is cooperative

A pause is a parked coroutine plus an engine flag. The frame loop never
stops. What stops is the game:

- `Engine::set_debug_scope(root)` names the subtree that is "the game" — the
  editor's mirror during play, the scene root under `balaur run`.
- While a script is paused the engine is **frozen**: `Stage::FixedUpdate`
  does not run (physics, `fixed_update`), and every script host skips
  `update`, `call_all` and `call_on` for instances inside the scope. Editor
  scripts live outside the scope and keep running.
- A breakpoint hits mid-batch. The host remembers the instances that had not
  yet run and finishes that batch on resume, before the next tick.

### The seam

Three methods on `ScriptHost`, defaulted so a backend without a debugger
compiles unchanged:

```rust
fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>>;
fn breakpoints(&self, path: &str) -> Vec<usize>;
fn paused(&self) -> Option<Pause>;
fn resume(&self, mode: StepMode);
```

`Pause` carries the node, path, line, reason and the call frames — each with
its function name, path, line and named locals as neutral values. The
`debugger` script module exposes the same four, plus `set_scope` /
`scope` and the `STEP_*` constants, so the editor's dock is plain Luau.

### Luau

- `lua_breakpoint` on the script's chunk function; the host keeps that
  function per file and re-applies the requested lines after a hot reload.
  A requested line lands on the next line with code; the landed line is what
  the editor marks.
- `debugbreak` records the hit and calls `lua_break`, but only when the
  thread is yieldable — a script running under a plain call from Rust
  (`on_free`, a module chunk) cannot be parked and is let through.
- While any breakpoint is set, `update` and `fixed_update` run through a
  coroutine like every other entry point. With none set the plain call path
  stays, so the no-debugger cost is unchanged.
- The host resumes coroutines through `lua_resume` itself, because `mlua`
  maps the break status to an error. Finished, yielded and failed threads
  settle exactly as before.
- Step over / into / out: `lua_singlestep` on the parked thread plus the
  `debugstep` callback comparing `lua_stackdepth` and the line to those at
  the pause.
- Frames and locals: `lua_getinfo` and `lua_getlocal` on the parked thread,
  moved to the main state and read as plain values. Requires debug level 2,
  which the shared compiler now sets for dev and export alike.

### Rune (phase 4)

A unit with breakpoints is called through `Vm::execute` and driven with
`VmExecution::step`, comparing the instruction pointer against the
breakpoint set after each step; units without breakpoints keep the
run-to-completion path. Pause holds the owned execution in host state.
Frames come from `call_frames` and the unit's debug info; locals show
`this`, the named arguments and the rest by slot index. The editor must boot
the mixed host to play a Rune game.

### Break on error (phase 5)

Luau's `debugprotectederror` fires only for errors a protected call in the
coroutine will catch. Breaking at the throw site for an uncaught error means
running each task under an `xpcall` wrapper and re-raising with
`lua_resumeerror` on resume. Designed, not built.

### Outside the editor (phase 6)

A Debug Adapter Protocol server thread for `balaur run --debug <port>`,
feeding commands into the frame loop at `Stage::First` the way the net plugin
does, over the same four host methods.

## 2. Phases

1. Engine freeze, the seam, Luau breakpoints with continue, frames, locals.
2. Editor: gutter clicks, current-line highlight, a Debugger dock tab,
   `F5` / `F10` / `F11` / `Shift+F11`.
3. Step over / into / out.
4. Rune stepping; the editor boots the mixed host.
5. Break on error.
6. DAP server.
