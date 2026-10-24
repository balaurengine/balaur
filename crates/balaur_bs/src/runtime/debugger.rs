// A debug build keeps its call stack in a shadow stack in linear memory and asks
// the host only at a breakpoint or while stepping; stopping is not returning yet.

use super::engine::{ImportCtx, Shared};
use super::host::CtxReader;
use super::values::{Reader, Value, rd32, read_value};
use crate::{Meta, Ty, words};
use std::collections::HashSet;
use std::sync::Arc;

/// What to do after a stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Continue,
    StepOver,
    StepInto,
    StepOut,
}

/// One function on the stack, with its variables as of its current line.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub func: String,
    pub line: u32,
    pub locals: Vec<(String, Value)>,
}

/// Why a script stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Breakpoint,
    Step,
    /// [`Debugger::pause`] asked, and this is the next statement it ran.
    Asked,
    /// The call trapped here; it is over, so this stop is looked at, not
    /// resumed ([`Runtime::error_stop`](super::Runtime::error_stop)).
    Error,
}

/// Where the script stopped. `frames[0]` is the innermost; changing a number
/// or bool in its `locals` changes the variable when the script resumes.
#[derive(Clone, Debug, PartialEq)]
pub struct Paused {
    pub line: u32,
    pub col: u32,
    pub frames: Vec<Frame>,
    pub reason: StopReason,
}

/// A stop held while the call it stopped is parked.
struct Parked {
    paused: Paused,
    depth: usize,
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Run,
    /// [`Debugger::pause`]: the next statement, whichever it is.
    Asked,
    Into {
        line: u32,
        depth: usize,
    },
    Over {
        line: u32,
        depth: usize,
    },
    Out {
        depth: usize,
    },
}

/// A frame of the shadow stack: function, its address, the statement it is on.
#[derive(Clone, Copy)]
pub(crate) struct Shadow {
    func: u32,
    at: u32,
    probe: Option<u32>,
}

type OnPause = Box<dyn FnMut(&mut Paused) -> Command>;

pub struct Debugger {
    meta: Arc<Meta>,
    breakpoints: HashSet<u32>,
    mode: Mode,
    on_pause: Option<OnPause>,
    dirty: bool,
    keep_locals: bool,
    /// Whether the running call can park: a stop then holds it rather than
    /// asking `on_pause` how to go on.
    pub(crate) park: bool,
    parked: Option<Parked>,
}

impl Debugger {
    pub(crate) fn new(meta: Arc<Meta>) -> Self {
        Debugger {
            meta,
            breakpoints: HashSet::new(),
            mode: Mode::Run,
            on_pause: None,
            dirty: true,
            keep_locals: false,
            park: false,
            parked: None,
        }
    }
    pub fn set_breakpoint(&mut self, line: u32) {
        self.dirty |= self.breakpoints.insert(line);
    }
    pub fn clear_breakpoint(&mut self, line: u32) {
        self.dirty |= self.breakpoints.remove(&line);
    }
    /// Stops at the next statement, wherever it is.
    pub fn pause(&mut self) {
        self.mode = Mode::Asked;
        self.dirty = true;
    }
    /// Every statement keeps its frame's variables current, not only those
    /// that stop, so the stack a trap leaves ([`StopReason::Error`]) shows
    /// them as they were. Off by default: it costs a store per variable per
    /// statement.
    pub fn keep_locals(&mut self, on: bool) {
        self.dirty |= self.keep_locals != on;
        self.keep_locals = on;
    }
    /// Every line with a breakpoint.
    pub fn breakpoints(&self) -> Vec<u32> {
        let mut lines: Vec<u32> = self.breakpoints.iter().copied().collect();
        lines.sort_unstable();
        lines
    }
    pub fn clear_breakpoints(&mut self) {
        self.dirty |= !self.breakpoints.is_empty();
        self.breakpoints.clear();
    }
    /// The stop a parked call is held at.
    pub fn parked(&self) -> Option<&Paused> {
        self.parked.as_ref().map(|p| &p.paused)
    }
    #[cfg_attr(
        not(feature = "wasmi"),
        allow(dead_code, reason = "only wasmi parks a call")
    )]
    pub(crate) fn is_parked(&self) -> bool {
        self.parked.is_some()
    }
    /// Lets the parked stop go under `cmd`; the flags the module needs, or
    /// `None` when nothing was parked.
    pub(crate) fn unpark(&mut self, cmd: Command) -> Option<i32> {
        let Parked { paused, depth } = self.parked.take()?;
        self.mode = mode_after(cmd, paused.line, depth);
        Some(self.flags())
    }
    /// Called at every stop; it decides how the script goes on. Without one, stops continue.
    pub fn on_pause(&mut self, f: impl FnMut(&mut Paused) -> Command + 'static) {
        self.on_pause = Some(Box::new(f));
    }

    /// The breakpoint table, if it changed: a byte per statement, set on the
    /// first statement of each breakpoint line so a line stops once per pass.
    pub(crate) fn take_table(&mut self) -> Option<(Vec<u8>, i32)> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let mut seen = HashSet::new();
        let table = self
            .meta
            .probes
            .iter()
            .map(|p| u8::from(self.breakpoints.contains(&p.line) && seen.insert((p.func, p.line))))
            .collect();
        Some((table, self.flags()))
    }
    /// The module's `__dbg_flags`: 1 while stepping, 2 to keep locals.
    pub(crate) fn flags(&self) -> i32 {
        i32::from(!matches!(self.mode, Mode::Run)) | (i32::from(self.keep_locals) << 1)
    }

    fn line(&self, probe: Option<u32>) -> u32 {
        probe.map_or(0, |p| self.meta.probes[p as usize].line)
    }

    /// The line a trap happened on: the innermost frame's statement.
    pub(crate) fn trap_line(&self, stack: &[Shadow]) -> Option<u32> {
        stack
            .last()
            .and_then(|f| f.probe)
            .map(|p| self.meta.probes[p as usize].line)
    }

    fn should_stop(&self, probe: u32, depth: usize) -> Option<StopReason> {
        let p = &self.meta.probes[probe as usize];
        let first_on_line = self
            .meta
            .probes
            .iter()
            .position(|q| q.func == p.func && q.line == p.line)
            == Some(probe as usize);
        let stepped = match self.mode {
            Mode::Run => false,
            Mode::Asked => true,
            Mode::Into { line, depth: d } => p.line != line || depth != d,
            Mode::Over { line, depth: d } => depth < d || (depth == d && p.line != line),
            Mode::Out { depth: d } => depth < d,
        };
        if first_on_line && self.breakpoints.contains(&p.line) {
            Some(StopReason::Breakpoint)
        } else if matches!(self.mode, Mode::Asked) {
            Some(StopReason::Asked)
        } else {
            stepped.then_some(StopReason::Step)
        }
    }

    pub(crate) fn snapshot(
        &self,
        m: &(impl Reader + ?Sized),
        stack: &[Shadow],
        reason: StopReason,
    ) -> Paused {
        let frames: Vec<Frame> = stack
            .iter()
            .rev()
            .map(|f| {
                let locals = f.probe.map_or(vec![], |p| {
                    let vars = &self.meta.probes[p as usize].vars;
                    vars.iter()
                        .map(|v| {
                            (
                                v.name.clone(),
                                read_value(&self.meta, m, &v.ty, (f.at + v.offset) as usize),
                            )
                        })
                        .collect()
                });
                let func = self
                    .meta
                    .func_names
                    .get(f.func as usize)
                    .cloned()
                    .unwrap_or_default();
                Frame {
                    func,
                    line: self.line(f.probe),
                    locals,
                }
            })
            .collect();
        let top = stack
            .last()
            .and_then(|f| f.probe)
            .map(|p| &self.meta.probes[p as usize]);
        Paused {
            line: top.map_or(0, |p| p.line),
            col: top.map_or(0, |p| p.col),
            frames,
            reason,
        }
    }

    // Numbers and bools the stop handler changed, as (address, bytes) to write back.
    fn edits(&self, stack: &[Shadow], before: &Paused, after: &Paused) -> Vec<(usize, [u8; 8])> {
        let mut out = vec![];
        for ((f, b), a) in stack.iter().rev().zip(&before.frames).zip(&after.frames) {
            let Some(p) = f.probe else { continue };
            for ((v, (_, old)), (_, new)) in self.meta.probes[p as usize]
                .vars
                .iter()
                .zip(&b.locals)
                .zip(&a.locals)
            {
                let bytes = match (&v.ty, new) {
                    _ if old == new => continue,
                    (Ty::Int, Value::Int(x)) | (Ty::Node, Value::Node(x)) => x.to_le_bytes(),
                    (Ty::Float, Value::Float(x)) => x.to_le_bytes(),
                    (Ty::Bool, Value::Bool(x)) => i64::from(*x).to_le_bytes(),
                    _ => continue,
                };
                out.push(((f.at + v.offset) as usize, bytes));
            }
        }
        out
    }
}

fn mode_after(cmd: Command, line: u32, depth: usize) -> Mode {
    match cmd {
        Command::Continue => Mode::Run,
        Command::StepInto => Mode::Into { line, depth },
        Command::StepOver => Mode::Over { line, depth },
        Command::StepOut => Mode::Out { depth },
    }
}

/// The shadow stack between `base` and `top`, outermost first.
pub(crate) fn shadow_stack(m: &(impl Reader + ?Sized), base: u32, top: u32) -> Vec<Shadow> {
    let mut out = vec![];
    let mut at = base;
    while at < top {
        let size = rd32(m, at as usize + 8);
        let probe = rd32(m, at as usize + 4) as i32;
        out.push(Shadow {
            func: rd32(m, at as usize),
            at,
            probe: u32::try_from(probe).ok(),
        });
        if size == 0 {
            break;
        }
        at += size;
    }
    out
}

pub(crate) fn on_probe(state: &Shared, ctx: &mut dyn ImportCtx, probe: u32) {
    let (Some(base), Some(top)) = (ctx.global(words::DBG_BASE), ctx.global(words::DBG_SP)) else {
        return;
    };
    let stack = shadow_stack(&CtxReader(&*ctx), base as u32, top as u32);
    let Some(reason) = state.borrow().debugger.should_stop(probe, stack.len()) else {
        return;
    };
    let before = state
        .borrow()
        .debugger
        .snapshot(&CtxReader(&*ctx), &stack, reason);
    let mut d = state.borrow_mut();
    if d.debugger.park {
        d.debugger.parked = Some(Parked {
            paused: before,
            depth: stack.len(),
        });
        return;
    }
    drop(d);
    let mut after = before.clone();
    let mut handler = state.borrow_mut().debugger.on_pause.take();
    let cmd = handler
        .as_mut()
        .map_or(Command::Continue, |f| f(&mut after));
    let mut h = state.borrow_mut();
    let d = &mut h.debugger;
    d.on_pause = handler;
    let edits = d.edits(&stack, &before, &after);
    d.mode = mode_after(cmd, before.line, stack.len());
    let flags = d.flags();
    drop(h);
    ctx.set_global(words::DBG_FLAGS, flags);
    for (at, bytes) in edits {
        ctx.write(at, &bytes);
    }
}
