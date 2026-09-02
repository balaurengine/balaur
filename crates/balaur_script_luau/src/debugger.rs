//! Breakpoints, stepping and frames over the raw Luau VM.
//!
//! `mlua` wraps none of Luau's debug hooks and reports a broken coroutine as
//! an error, so this file talks to `mlua::ffi`: it installs the break and
//! step callbacks, patches breakpoints into a chunk's protos, resumes
//! coroutines itself, and reads frames and locals off a parked thread.

use std::cell::Cell;
use std::ffi::{c_char, c_int, CStr};

use balaur_script::{Frame, PauseReason, StepMode, Value};
use mlua::ffi;
use mlua::{Function, Lua, MultiValue, Thread};

/// What a hook recorded before parking the thread.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub(crate) line: usize,
    pub(crate) reason: PauseReason,
}

/// One coroutine resume, however it ended.
pub(crate) enum Outcome {
    Finished(MultiValue),
    Yielded(MultiValue),
    Broke(Hit),
    Failed(mlua::Error),
}

#[derive(Clone, Copy)]
struct StepPlan {
    thread: *mut ffi::lua_State,
    mode: StepMode,
    depth: c_int,
    line: c_int,
}

thread_local! {
    /// The hit a hook just recorded, read back by the resume that saw the break.
    static HIT: Cell<Option<Hit>> = const { Cell::new(None) };
    static STEP: Cell<Option<StepPlan>> = const { Cell::new(None) };
    /// A thread leaving a breakpoint re-executes the BREAK it stopped on; the
    /// hook lets that one through.
    static LEAVING: Cell<*mut ffi::lua_State> = const { Cell::new(std::ptr::null_mut()) };
}

/// Nesting past which a local's table is shown as `{…}`; a script's tables
/// refer back to each other freely.
const LOCAL_DEPTH: usize = 4;

/// Registry slot the hook leaves the parked thread's frames in.
const FRAMES_KEY: &CStr = c"balaur.debugger.frames";

unsafe extern "C-unwind" fn on_break(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug) {
    if LEAVING.with(Cell::get) == state {
        LEAVING.with(|l| l.set(std::ptr::null_mut()));
        return;
    }
    // A plain call from Rust (`on_free`, a module chunk) cannot be parked.
    if ffi::lua_isyieldable(state) == 0 {
        return;
    }
    park(state, ar, PauseReason::Breakpoint);
}

unsafe extern "C-unwind" fn on_step(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug) {
    let Some(plan) = STEP.with(Cell::get) else {
        return;
    };
    if plan.thread != state || ffi::lua_isyieldable(state) == 0 {
        return;
    }
    let depth = ffi::lua_stackdepth(state);
    let line = (*ar).currentline;
    let arrived = match plan.mode {
        StepMode::Continue => false,
        StepMode::Into => line != plan.line || depth != plan.depth,
        StepMode::Over => depth < plan.depth || (depth == plan.depth && line != plan.line),
        StepMode::Out => depth < plan.depth,
    };
    if !arrived {
        return;
    }
    STEP.with(|s| s.set(None));
    ffi::lua_singlestep(state, 0);
    park(state, ar, PauseReason::Step);
}

unsafe fn park(state: *mut ffi::lua_State, ar: *mut ffi::lua_Debug, reason: PauseReason) {
    let line = usize::try_from((*ar).currentline).unwrap_or(0);
    capture_frames(state);
    HIT.with(|h| h.set(Some(Hit { line, reason })));
    ffi::lua_break(state);
}

/// Every Lua frame with its named locals, as an array of tables left in the
/// registry. Read here rather than after the break: only inside the hook is
/// the frame's pc the instruction it stopped on, so only here do
/// `lua_getinfo` and `lua_getlocal` answer for the current line.
unsafe fn capture_frames(state: *mut ffi::lua_State) {
    ffi::lua_createtable(state, 0, 0);
    let mut count: c_int = 0;
    for level in 0..ffi::lua_stackdepth(state) {
        let mut ar: ffi::lua_Debug = std::mem::zeroed();
        if ffi::lua_getinfo(state, level, c"sln".as_ptr(), &raw mut ar) == 0 {
            break;
        }
        if first_byte(ar.what) == Some(b'C') {
            continue;
        }
        ffi::lua_createtable(state, 0, 4);
        ffi::lua_pushstring(
            state,
            if ar.name.is_null() {
                c"?".as_ptr()
            } else {
                ar.name
            },
        );
        ffi::lua_setfield(state, -2, c"function".as_ptr());
        ffi::lua_pushstring(state, ar.source);
        ffi::lua_setfield(state, -2, c"path".as_ptr());
        ffi::lua_pushnumber(state, f64::from(ar.currentline));
        ffi::lua_setfield(state, -2, c"line".as_ptr());
        ffi::lua_createtable(state, 0, 0);
        let mut n = 1;
        loop {
            let name = ffi::lua_getlocal(state, level, n);
            if name.is_null() {
                break;
            }
            // Luau's unnamed temporaries.
            if first_byte(name) == Some(b'(') {
                ffi::lua_pop(state, 1);
            } else {
                ffi::lua_setfield(state, -2, name);
            }
            n += 1;
        }
        ffi::lua_setfield(state, -2, c"locals".as_ptr());
        count += 1;
        ffi::lua_rawseti(state, -2, ffi::lua_Integer::from(count));
    }
    ffi::lua_setfield(state, ffi::LUA_REGISTRYINDEX, FRAMES_KEY.as_ptr());
}

/// Install the hooks on the VM behind `lua`. Once per state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    unsafe {
        lua.exec_raw::<()>((), |state| {
            let callbacks = ffi::lua_callbacks(state);
            (*callbacks).debugbreak = Some(on_break);
            (*callbacks).debugstep = Some(on_step);
        })
    }
}

/// Patch one breakpoint into `chunk`'s protos, nested functions included.
/// Returns the line it landed on: `None` when no line at or after `line`
/// has code.
pub(crate) fn set_breakpoint(
    lua: &Lua,
    chunk: &Function,
    line: usize,
    enabled: bool,
) -> mlua::Result<Option<usize>> {
    let line = c_int::try_from(line).unwrap_or(c_int::MAX);
    let landed: f64 = unsafe {
        lua.exec_raw::<f64>(chunk.clone(), |state| {
            let landed = ffi::lua_breakpoint(state, -1, line, c_int::from(enabled));
            ffi::lua_pop(state, 1);
            ffi::lua_pushnumber(state, f64::from(landed));
        })?
    };
    Ok((landed > 0.0).then_some(landed as usize))
}

/// Resume `thread` with `args`, saying how it stopped.
pub(crate) fn resume(lua: &Lua, thread: &Thread, args: MultiValue) -> Outcome {
    let nargs = c_int::try_from(args.len()).unwrap_or(c_int::MAX);
    let thread_state = thread.state();
    // The status rides back as the first result, ahead of what the thread
    // returned, yielded or raised.
    let moved: mlua::Result<MultiValue> = unsafe {
        lua.exec_raw::<MultiValue>(args, |state| {
            ffi::lua_checkstack(thread_state, nargs + 1);
            ffi::lua_xmove(state, thread_state, nargs);
            let mut nresults = 0;
            let status = ffi::lua_resumex(thread_state, state, nargs, &raw mut nresults);
            ffi::lua_pushnumber(state, f64::from(status));
            match status {
                ffi::LUA_OK | ffi::LUA_YIELD => {
                    ffi::lua_checkstack(state, nresults + 1);
                    ffi::lua_xmove(thread_state, state, nresults);
                }
                ffi::LUA_BREAK => {}
                _ => {
                    ffi::lua_checkstack(state, 2);
                    ffi::lua_xmove(thread_state, state, 1);
                    ffi::luaL_traceback(state, thread_state, std::ptr::null(), 0);
                }
            }
        })
    };
    let mut values = match moved {
        Ok(values) => values.into_vec(),
        Err(err) => return Outcome::Failed(err),
    };
    if values.is_empty() {
        return Outcome::Failed(mlua::Error::runtime("the resume reported no status"));
    }
    let status = match values.remove(0) {
        mlua::Value::Number(n) => n as c_int,
        mlua::Value::Integer(i) => i as c_int,
        _ => -1,
    };
    match status {
        ffi::LUA_OK => Outcome::Finished(MultiValue::from_vec(values)),
        ffi::LUA_YIELD => Outcome::Yielded(MultiValue::from_vec(values)),
        ffi::LUA_BREAK => Outcome::Broke(HIT.with(Cell::take).unwrap_or(Hit {
            line: 0,
            reason: PauseReason::Breakpoint,
        })),
        _ => {
            let text =
                |v: Option<&mlua::Value>| v.and_then(|v| v.to_string().ok()).unwrap_or_default();
            let message = text(values.first());
            let traceback = text(values.get(1));
            Outcome::Failed(mlua::Error::runtime(format!("{message}\n{traceback}")))
        }
    }
}

/// Arm single-stepping on a parked thread: its next resume runs until
/// `mode` says stop, measured from the depth and line it is parked at.
pub(crate) fn begin_step(thread: &Thread, mode: StepMode, line: usize) {
    let state = thread.state();
    let depth = unsafe { ffi::lua_stackdepth(state) };
    let line = c_int::try_from(line).unwrap_or(0);
    STEP.with(|s| {
        s.set(Some(StepPlan {
            thread: state,
            mode,
            depth,
            line,
        }));
    });
    unsafe { ffi::lua_singlestep(state, 1) };
}

/// The thread is about to leave the breakpoint it is parked on.
pub(crate) fn leaving_breakpoint(thread: &Thread) {
    LEAVING.with(|l| l.set(thread.state()));
}

/// A parked thread being dropped must leave nothing armed for the next
/// thread to reuse its address.
pub(crate) fn forget(thread: &Thread) {
    let state = thread.state();
    if STEP
        .with(Cell::get)
        .is_some_and(|plan| plan.thread == state)
    {
        STEP.with(|s| s.set(None));
    }
    if LEAVING.with(Cell::get) == state {
        LEAVING.with(|l| l.set(std::ptr::null_mut()));
    }
    unsafe { ffi::lua_singlestep(state, 0) };
}

/// The frames the hook captured for the thread that just broke, innermost
/// first. Takes them out of the registry.
pub(crate) fn frames(lua: &Lua) -> Vec<Frame> {
    let table: mlua::Result<Option<mlua::Table>> = unsafe {
        lua.exec_raw::<Option<mlua::Table>>((), |state| {
            ffi::lua_getfield(state, ffi::LUA_REGISTRYINDEX, FRAMES_KEY.as_ptr());
            ffi::lua_pushnil(state);
            ffi::lua_setfield(state, ffi::LUA_REGISTRYINDEX, FRAMES_KEY.as_ptr());
        })
    };
    let Ok(Some(table)) = table else {
        return Vec::new();
    };
    let mut frames = Vec::new();
    for frame in table.sequence_values::<mlua::Table>().flatten() {
        let source: String = frame.get("path").unwrap_or_default();
        let mut locals = Vec::new();
        if let Ok(names) = frame.get::<mlua::Table>("locals") {
            for (name, value) in names.pairs::<String, mlua::Value>().flatten() {
                if let Some(plain) = shallow(&value, LOCAL_DEPTH) {
                    locals.push((name, plain));
                }
            }
        }
        locals.sort_by(|a, b| a.0.cmp(&b.0));
        frames.push(Frame {
            function: frame.get("function").unwrap_or_else(|_| "?".into()),
            path: source.strip_prefix('@').unwrap_or(&source).to_string(),
            line: frame.get::<f64>("line").map_or(0, |l| l.max(0.0) as usize),
            locals,
        });
    }
    frames
}

/// A local as a plain value, cut off at `depth` so a cyclic table ends in
/// `{…}` instead of a stack overflow.
fn shallow(v: &mlua::Value, depth: usize) -> Option<Value> {
    let mlua::Value::Table(t) = v else {
        return crate::env::to_plain(v);
    };
    if depth == 0 {
        return Some(Value::Str("{…}".into()));
    }
    let mut map = Vec::new();
    let _ = t.for_each(|k: mlua::Value, v: mlua::Value| {
        let key = match &k {
            mlua::Value::String(k) => match k.to_str() {
                Ok(k) => k.to_string(),
                Err(_) => return Ok(()),
            },
            mlua::Value::Integer(i) => i.to_string(),
            _ => return Ok(()),
        };
        if let Some(value) = shallow(&v, depth - 1) {
            map.push((key, value));
        }
        Ok(())
    });
    Some(Value::Map(map))
}

unsafe fn first_byte(s: *const c_char) -> Option<u8> {
    if s.is_null() {
        None
    } else {
        Some(*s.cast::<u8>())
    }
}
