// A debug build keeps its call stack in a shadow stack in linear memory and asks
// the host only at a breakpoint or while stepping; stopping is not returning yet.

use super::engine::{ImportCtx, Shared};
use super::host::CtxReader;
use super::values::{Reader, Value, rd32, read_value};
use crate::{Meta, Probe, Ty, words};
use rustc_hash::FxHashMap;
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
    /// The file it is in: a stack crosses the units of its instance.
    pub file: String,
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

/// A frame of the shadow stack: the unit it is in, its function, its address,
/// the statement it is on.
#[derive(Clone, Copy)]
pub(crate) struct Shadow {
    unit: usize,
    func: u32,
    at: u32,
    probe: Option<u32>,
}

type OnPause = Box<dyn FnMut(&mut Paused) -> Command>;

/// Each debug unit's breakpoint table, by its index in the instance.
pub(crate) type Tables = Vec<(usize, Vec<u8>)>;

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches of a debugger's state, not one state enum"
)]
pub struct Debugger {
    /// Every unit of the instance by its index, and its file.
    units: Vec<(String, Arc<Meta>)>,
    /// Whether any unit was built with probes.
    probed: bool,
    breakpoints: FxHashMap<usize, HashSet<u32>>,
    mode: Mode,
    on_pause: Option<OnPause>,
    dirty: bool,
    keep_locals: bool,
    /// Whether the running call can park: a stop then holds it rather than
    /// asking `on_pause` how to go on.
    pub(crate) park: bool,
    parked: Option<Parked>,
    /// A probe just parked its call: the engine traps out of it.
    parking: bool,
}

impl Debugger {
    pub(crate) fn new(key: &str, meta: Arc<Meta>) -> Self {
        let mut debugger = Debugger {
            units: vec![],
            probed: false,
            breakpoints: FxHashMap::default(),
            mode: Mode::Run,
            on_pause: None,
            dirty: true,
            keep_locals: false,
            park: false,
            parked: None,
            parking: false,
        };
        debugger.add_unit(key, meta);
        debugger
    }

    /// The next unit of the instance.
    pub(crate) fn add_unit(&mut self, key: &str, meta: Arc<Meta>) {
        self.probed |= meta.debug;
        self.dirty |= meta.debug;
        self.units.push((key.to_string(), meta));
    }

    /// Whether any unit of the instance was built with probes.
    pub fn probed(&self) -> bool {
        self.probed
    }

    /// The index the file at `key` has in the instance.
    pub fn unit_named(&self, key: &str) -> Option<usize> {
        self.units.iter().position(|(file, _)| file == key)
    }

    pub fn set_breakpoint(&mut self, line: u32) {
        self.set_breakpoint_in(0, line);
    }
    /// A breakpoint on line `line` of unit `unit`'s file.
    pub fn set_breakpoint_in(&mut self, unit: usize, line: u32) {
        self.dirty |= self.breakpoints.entry(unit).or_default().insert(line);
    }
    pub fn clear_breakpoint(&mut self, line: u32) {
        if let Some(lines) = self.breakpoints.get_mut(&0) {
            self.dirty |= lines.remove(&line);
        }
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
        let mut lines: Vec<u32> = self
            .breakpoints
            .get(&0)
            .map(|lines| lines.iter().copied().collect())
            .unwrap_or_default();
        lines.sort_unstable();
        lines
    }
    pub fn clear_breakpoints(&mut self) {
        self.clear_breakpoints_in(0);
    }
    /// No breakpoint in unit `unit`'s file.
    pub fn clear_breakpoints_in(&mut self, unit: usize) {
        if let Some(lines) = self.breakpoints.remove(&unit) {
            self.dirty |= !lines.is_empty();
        }
    }
    /// The stop a parked call is held at.
    pub fn parked(&self) -> Option<&Paused> {
        self.parked.as_ref().map(|p| &p.paused)
    }
    /// Whether a probe just parked its call, once.
    #[cfg_attr(
        not(feature = "wasmi"),
        allow(dead_code, reason = "only wasmi parks a call")
    )]
    pub(crate) fn take_parking(&mut self) -> bool {
        std::mem::take(&mut self.parking)
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

    /// Each debug unit's breakpoint table, if anything changed: a byte per
    /// statement, set on the first statement of each breakpoint line so a
    /// line stops once per pass.
    pub(crate) fn take_tables(&mut self) -> Option<(Tables, i32)> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let tables = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, (_, meta))| meta.debug)
            .map(|(unit, (_, meta))| {
                let lines = self.breakpoints.get(&unit);
                let mut seen = HashSet::new();
                let table = meta
                    .probes
                    .iter()
                    .map(|probe| {
                        let set = lines.is_some_and(|lines| lines.contains(&probe.line));
                        u8::from(set && seen.insert((probe.func, probe.line)))
                    })
                    .collect();
                (unit, table)
            })
            .collect();
        Some((tables, self.flags()))
    }
    /// The module's `__dbg_flags`: 1 while stepping, 2 to keep locals.
    pub(crate) fn flags(&self) -> i32 {
        i32::from(!matches!(self.mode, Mode::Run)) | (i32::from(self.keep_locals) << 1)
    }

    fn probe(&self, unit: usize, probe: u32) -> Option<&Probe> {
        self.units
            .get(unit)
            .and_then(|(_, meta)| meta.probes.get(probe as usize))
    }

    /// The line a trap happened on: the innermost frame's statement.
    pub(crate) fn trap_line(&self, stack: &[Shadow]) -> Option<u32> {
        let top = stack.last()?;
        self.probe(top.unit, top.probe?).map(|found| found.line)
    }

    fn should_stop(&self, unit: usize, probe: u32, depth: usize) -> Option<StopReason> {
        let here = self.probe(unit, probe)?;
        let first_on_line = self.units[unit]
            .1
            .probes
            .iter()
            .position(|other| other.func == here.func && other.line == here.line)
            == Some(probe as usize);
        let stepped = match self.mode {
            Mode::Run => false,
            Mode::Asked => true,
            Mode::Into { line, depth: from } => here.line != line || depth != from,
            Mode::Over { line, depth: from } => {
                depth < from || (depth == from && here.line != line)
            }
            Mode::Out { depth: from } => depth < from,
        };
        let on_breakpoint = self
            .breakpoints
            .get(&unit)
            .is_some_and(|lines| lines.contains(&here.line));
        if first_on_line && on_breakpoint {
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
                let (file, meta) = &self.units[f.unit];
                let probe = f.probe.and_then(|p| meta.probes.get(p as usize));
                let locals = probe.map_or(vec![], |p| {
                    p.vars
                        .iter()
                        .map(|v| {
                            (
                                v.name.clone(),
                                read_value(meta, m, &v.ty, (f.at + v.offset) as usize),
                            )
                        })
                        .collect()
                });
                let func = meta
                    .func_names
                    .get(f.func as usize)
                    .cloned()
                    .unwrap_or_default();
                Frame {
                    func,
                    file: file.clone(),
                    line: probe.map_or(0, |p| p.line),
                    locals,
                }
            })
            .collect();
        let top = stack.last().and_then(|f| self.probe(f.unit, f.probe?));
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
            let Some(p) = f.probe.and_then(|p| self.probe(f.unit, p)) else {
                continue;
            };
            for ((v, (_, old)), (_, new)) in p.vars.iter().zip(&b.locals).zip(&a.locals) {
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
            unit: rd32(m, at as usize + 12) as usize,
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

pub(crate) fn on_probe(state: &Shared, ctx: &mut dyn ImportCtx, unit: usize, probe: u32) {
    // One stop at a time: a call made while another is parked passes by.
    if state.borrow().debugger.parked.is_some() {
        return;
    }
    let (Some(base), Some(top)) = (ctx.global(words::DBG_BASE), ctx.global(words::DBG_SP)) else {
        return;
    };
    let stack = shadow_stack(&CtxReader(&*ctx), base as u32, top as u32);
    let reason = state
        .borrow()
        .debugger
        .should_stop(unit, probe, stack.len());
    let Some(reason) = reason else {
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
        d.debugger.parking = true;
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
