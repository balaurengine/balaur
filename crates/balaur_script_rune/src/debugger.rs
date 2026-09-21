//! Breakpoints and stepping over Rune's executor.
//!
//! The VM stops itself: it is handed a set of instruction pointers and halts
//! before any of them, so it runs at full speed in between. A plain
//! breakpoint costs the script nothing until it is hit. Stepping hands over
//! the first instruction of every line instead, so a step over a call that
//! runs a hundred thousand instructions is one resume, not a hundred thousand
//! round trips through the executor.
//!
//! A unit with nothing to stop for gets no set at all and keeps the
//! run-to-completion call.

use std::collections::HashMap;
use std::sync::Arc;

use balaur_script::{Frame, PauseReason, StepMode};
use rune::runtime::debug::DebugArgs;
use rune::runtime::{HaltSet, InstAddress, Unit, Vm, VmError, VmExecution, VmResult};

/// Where a unit's instructions sit in its source, and where its functions
/// start.
pub(crate) struct Lines {
    line_of_ip: HashMap<usize, usize>,
    /// Function start ips, ascending.
    starts: Vec<usize>,
    /// The first instruction of every line of every function: everywhere a
    /// step or a pause may come to rest. Handed to the VM whole, so it runs
    /// between two lines without asking.
    stops: Arc<HaltSet>,
}

impl Lines {
    pub(crate) fn of(unit: &Unit, source: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let mut line_of_ip = HashMap::new();
        let mut starts = Vec::new();
        if let Some(info) = unit.debug_info() {
            for (ip, inst) in &info.instructions {
                let byte = inst.span.start.into_usize();
                line_of_ip.insert(*ip, line_starts.partition_point(|&s| s <= byte));
            }
            starts.extend(info.functions_rev.keys().copied());
        }
        starts.sort_unstable();
        let mut lines = Self {
            line_of_ip,
            starts,
            stops: Arc::new(HaltSet::new()),
        };
        lines.stops = Arc::new(lines.line_start_ips());
        lines
    }

    /// The first instruction of each line, per function, so a line reached
    /// from two call sites stops in both.
    fn line_start_ips(&self) -> HaltSet {
        let mut first: HashMap<(Option<usize>, usize), usize> = HashMap::new();
        for (ip, line) in &self.line_of_ip {
            if *line == 0 {
                continue;
            }
            first
                .entry((self.function_start(*ip), *line))
                .and_modify(|at| *at = (*at).min(*ip))
                .or_insert(*ip);
        }
        HaltSet::from_ips(first.into_values()).unwrap_or_default()
    }

    /// Everywhere a step or a pause may stop.
    pub(crate) fn stops(&self) -> Arc<HaltSet> {
        self.stops.clone()
    }

    /// The source line of an instruction; 0 for one with no span.
    pub(crate) fn line(&self, ip: usize) -> usize {
        self.line_of_ip.get(&ip).copied().unwrap_or(0)
    }

    /// The start ip of the function an instruction belongs to.
    pub(crate) fn function_start(&self, ip: usize) -> Option<usize> {
        match self.starts.binary_search(&ip) {
            Ok(i) => Some(self.starts[i]),
            Err(0) => None,
            Err(i) => Some(self.starts[i - 1]),
        }
    }

    /// Where a breakpoint asked for on `line` lands — the next line with
    /// code — and the first instruction of that line in each function.
    pub(crate) fn breakpoint(&self, line: usize) -> Option<(usize, Vec<usize>)> {
        let landed = self
            .line_of_ip
            .values()
            .copied()
            .filter(|l| *l >= line)
            .min()?;
        let mut first: HashMap<Option<usize>, usize> = HashMap::new();
        for (ip, l) in &self.line_of_ip {
            if *l != landed {
                continue;
            }
            first
                .entry(self.function_start(*ip))
                .and_modify(|m| *m = (*m).min(*ip))
                .or_insert(*ip);
        }
        Some((landed, first.into_values().collect()))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub(crate) line: usize,
    pub(crate) reason: PauseReason,
}

/// One driven execution, however it ended.
pub(crate) enum Outcome {
    Finished(rune::Value),
    Broke(Hit),
    /// It threw. The line is where, so a pause can point at it.
    Failed {
        error: VmError,
        line: usize,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct StepPlan {
    pub(crate) mode: StepMode,
    pub(crate) depth: usize,
    pub(crate) line: usize,
}

/// What [`run`] stops for besides the breakpoint set.
#[derive(Clone, Copy, Default)]
pub(crate) struct Stops {
    pub(crate) plan: Option<StepPlan>,
    /// Stop at the first line this execution reaches — the Pause button.
    pub(crate) break_next: bool,
}

/// Drive `exec` until a breakpoint, a stop in `stops`, an error or the end.
///
/// `halts` is where the VM gives control back: the breakpoint set on its own
/// when nothing is stepping, every line start when something is. `leaving` is
/// the instruction it is parked on, which runs without breaking again.
pub(crate) fn run(
    exec: &mut VmExecution<Vm>,
    lines: &Lines,
    halts: &Arc<HaltSet>,
    breakpoints: &Arc<HaltSet>,
    stops: Stops,
    leaving: Option<usize>,
) -> Outcome {
    exec.vm_mut().set_halts(Some(halts.clone()));
    exec.vm_mut().set_resuming(leaving);
    loop {
        match exec.run_to_break() {
            VmResult::Ok(Some(value)) => return Outcome::Finished(value),
            VmResult::Err(err) => {
                // The ip has already moved past the instruction that threw.
                return Outcome::Failed {
                    error: err,
                    line: lines.line(exec.vm().last_ip()),
                };
            }
            VmResult::Ok(None) => {}
        }
        let ip = exec.vm().ip();
        let line = lines.line(ip);
        let depth = exec.vm().call_frames().len();
        if stops.break_next && line > 0 {
            return Outcome::Broke(Hit {
                line,
                reason: PauseReason::Pause,
            });
        }
        if let Some(plan) = stops.plan {
            let arrived = match plan.mode {
                StepMode::Continue => false,
                StepMode::Into => line != plan.line || depth != plan.depth,
                StepMode::Over => depth < plan.depth || (depth == plan.depth && line != plan.line),
                StepMode::Out => depth < plan.depth,
            };
            if arrived && line > 0 {
                return Outcome::Broke(Hit {
                    line,
                    reason: PauseReason::Step,
                });
            }
        }
        if breakpoints.contains(ip) {
            return Outcome::Broke(Hit {
                line,
                reason: PauseReason::Breakpoint,
            });
        }
        // A halt the predicate did not want: go on past it.
        exec.vm_mut().set_resuming(Some(ip));
    }
}

/// The frames of a parked execution, innermost first. Function names come
/// from the unit's debug info; the innermost frame's named arguments are its
/// locals, which is every name the slot VM keeps.
pub(crate) fn frames(
    exec: &VmExecution<Vm>,
    lines: &Lines,
    key: &str,
    top_line: usize,
) -> Vec<Frame> {
    let vm = exec.vm();
    let info = vm.unit().debug_info();
    let mut ips = vec![vm.ip()];
    // A caller is saved with its return address, one past the call.
    ips.extend(
        vm.call_frames()
            .iter()
            .rev()
            .filter(|f| f.ip > 0)
            .map(|f| f.ip - 1),
    );
    let mut frames = Vec::new();
    for (level, ip) in ips.iter().enumerate() {
        let signature = lines
            .function_start(*ip)
            .and_then(|start| info?.functions_rev.get(&start))
            .and_then(|hash| info?.functions.get(hash));
        let function = signature.map_or_else(|| "?".to_string(), |s| s.path.to_string());
        let mut locals = Vec::new();
        if level == 0
            && let Some(DebugArgs::Named(names)) = signature.map(|s| &s.args)
            && let Ok(slots) = vm.stack().slice_at(InstAddress::ZERO, names.len())
        {
            for (name, value) in names.iter().zip(slots) {
                if let Some(plain) = crate::value::to_plain(value) {
                    locals.push((name.to_string(), plain));
                }
            }
        }
        frames.push(Frame {
            function,
            path: key.to_string(),
            line: if level == 0 {
                top_line
            } else {
                lines.line(*ip)
            },
            locals,
        });
    }
    frames
}
