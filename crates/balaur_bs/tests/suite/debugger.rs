use super::run_with;
use balaur_bs::Options;
use balaur_bs::runtime::{Command, HostFns, Paused, StopReason, Value};
use std::cell::RefCell;
use std::rc::Rc;

const SCRIPT: &str = "pub struct Mover { x: float, steps: int }
impl Mover {
    pub fn walk(n: int) -> float {
        for i in 0..n {
            self.x += stride(i);
            self.steps += 1;
        }
        self.x
    }
}
fn stride(i: int) -> float {
    let s = i * 2;
    s + 0.5
}
pub fn crash(i: int) -> float {
    let a = [1.0, 2.0];
    a[i]
}
";

fn debug_runtime() -> balaur_bs::runtime::Runtime {
    run_with(
        SCRIPT,
        HostFns::default(),
        &Options {
            debug: true,
            ..Options::default()
        },
    )
}

fn local(p: &Paused, frame: usize, name: &str) -> Value {
    p.frames[frame]
        .locals
        .iter()
        .find(|(n, _)| n == name)
        .map_or(Value::Void, |(_, v)| v.clone())
}

#[test]
fn a_breakpoint_stops_with_the_stack_and_its_variables() {
    let mut rt = debug_runtime();
    let stops = Rc::new(RefCell::new(vec![]));
    let seen = stops.clone();
    rt.debugger().set_breakpoint(12);
    rt.debugger().on_pause(move |p| {
        seen.borrow_mut().push(p.clone());
        Command::Continue
    });
    let mut m = rt.object("Mover", &[]).unwrap();
    rt.call_method(&mut m, "walk", &[Value::Int(2)]).unwrap();
    let stops = stops.borrow();
    assert_eq!(stops.len(), 2);
    let p = &stops[1];
    assert_eq!(p.line, 12);
    assert_eq!(
        p.frames
            .iter()
            .map(|f| (f.func.as_str(), f.line))
            .collect::<Vec<_>>(),
        [("stride", 12), ("Mover.walk", 5)]
    );
    assert_eq!(local(p, 0, "i"), Value::Int(1));
    assert_eq!(local(p, 1, "i"), Value::Int(1));
    assert_eq!(local(p, 1, "n"), Value::Int(2));
    assert_eq!(
        local(p, 1, "self"),
        Value::Struct(
            "Mover".into(),
            vec![
                ("x".into(), Value::Float(0.5)),
                ("steps".into(), Value::Int(1))
            ]
        )
    );
}

#[test]
fn stepping_over_into_and_out() {
    let mut rt = debug_runtime();
    let lines = Rc::new(RefCell::new(vec![]));
    let seen = lines.clone();
    let mut plan = vec![
        Command::StepOver,
        Command::StepInto,
        Command::StepOver,
        Command::StepOut,
        Command::Continue,
    ]
    .into_iter();
    rt.debugger().set_breakpoint(4);
    rt.debugger().on_pause(move |p| {
        seen.borrow_mut().push((p.frames[0].func.clone(), p.line));
        plan.next().unwrap_or(Command::Continue)
    });
    let mut m = rt.object("Mover", &[]).unwrap();
    rt.call_method(&mut m, "walk", &[Value::Int(1)]).unwrap();
    let want = [
        ("Mover.walk", 4),
        ("Mover.walk", 5),
        ("stride", 12),
        ("stride", 13),
        ("Mover.walk", 6),
    ];
    let got: Vec<(String, u32)> = lines.borrow().clone();
    assert_eq!(
        got,
        want.iter()
            .map(|(f, l)| ((*f).to_string(), *l))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_stop_can_change_a_variable() {
    let mut rt = debug_runtime();
    rt.debugger().set_breakpoint(13);
    rt.debugger().on_pause(|p| {
        for (n, v) in &mut p.frames[0].locals {
            if n == "s" {
                *v = Value::Int(100);
            }
        }
        Command::Continue
    });
    let mut m = rt.object("Mover", &[]).unwrap();
    assert_eq!(
        rt.call_method(&mut m, "walk", &[Value::Int(1)]).unwrap(),
        Value::Float(100.5)
    );
}

#[test]
fn a_trap_reports_its_line() {
    let mut rt = debug_runtime();
    let e = rt.call("crash", &[Value::Int(5)]).unwrap_err();
    assert_eq!(e.line, Some(17));
    assert_eq!(
        rt.call("crash", &[Value::Int(1)]).unwrap(),
        Value::Float(2.0)
    );
}

#[test]
fn a_trap_leaves_the_stack_it_died_with() {
    let mut rt = debug_runtime();
    rt.debugger().keep_locals(true);
    rt.call("crash", &[Value::Int(5)]).unwrap_err();
    let p = rt.error_stop().expect("a debug build keeps the stack");
    assert_eq!((p.line, p.reason), (17, StopReason::Error));
    assert_eq!(p.frames[0].func, "crash");
    assert_eq!(local(&p, 0, "i"), Value::Int(5));
    assert_eq!(
        local(&p, 0, "a"),
        Value::Array(vec![Value::Float(1.0), Value::Float(2.0)])
    );
    assert!(rt.error_stop().is_none(), "taken once");
    rt.call("crash", &[Value::Int(1)]).unwrap();
    assert!(rt.error_stop().is_none());
}

#[test]
fn release_builds_carry_no_probes() {
    let release = balaur_bs::compile(SCRIPT, &Options::default()).unwrap();
    let debug = balaur_bs::compile(
        SCRIPT,
        &Options {
            debug: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(release.meta.probes.is_empty());
    assert!(!debug.meta.probes.is_empty());
    assert!(release.wasm.len() < debug.wasm.len());
}

// On wasmi a stop parks the call: the host gets control back mid-method.
#[cfg(feature = "wasmi")]
mod parked {
    use super::{SCRIPT, local};
    use balaur_bs::runtime::{
        Command, EngineKind, HostFns, Limits, Runtime, Step, StopReason, Value,
    };
    use balaur_bs::{Options, compile};

    fn on_wasmi() -> Runtime {
        on_wasmi_with(false)
    }

    // `direct`: the browser's module shape, whose every import rides an
    // entry point that answers one i64, the probe included.
    fn on_wasmi_with(direct: bool) -> Runtime {
        let options = Options {
            debug: true,
            direct_imports: direct,
            ..Options::default()
        };
        let c = compile(SCRIPT, &options).unwrap();
        let limits = Limits {
            engine: EngineKind::Wasmi,
            ..Limits::default()
        };
        Runtime::new(&c, HostFns::default(), limits).unwrap()
    }

    fn paused(step: Step) -> balaur_bs::runtime::Paused {
        match step {
            Step::Paused(p) => p,
            other => panic!("{other:?} instead of stopping"),
        }
    }

    #[test]
    fn a_stop_parks_the_call_until_it_is_resumed() {
        let mut rt = on_wasmi();
        let mut m = rt.object("Mover", &[]).unwrap();
        rt.debugger().set_breakpoint(5);
        let p = paused(
            rt.call_method_steppable(&mut m, "walk", &[Value::Int(2)])
                .unwrap(),
        );
        assert_eq!((p.line, p.reason), (5, StopReason::Breakpoint));
        assert_eq!(local(&p, 0, "i"), Value::Int(0));
        assert!(rt.paused().is_some());
        assert!(
            rt.call("crash", &[Value::Int(0)]).is_err(),
            "ran beside a parked call"
        );

        let p = paused(rt.resume(Command::StepOver, &mut m).unwrap());
        assert_eq!((p.line, p.reason), (6, StopReason::Step));
        rt.debugger().clear_breakpoint(5);
        let done = rt.resume(Command::Continue, &mut m).unwrap();
        assert_eq!(done, Step::Done(Value::Float(3.0)));
        assert_eq!(rt.get(&m, "steps"), Some(Value::Int(2)));
        assert!(rt.paused().is_none());
        assert_eq!(
            rt.call("crash", &[Value::Int(1)]).unwrap(),
            Value::Float(2.0)
        );
    }

    #[test]
    fn a_direct_module_parks_and_resumes_as_the_ordinary_one_does() {
        let mut rt = on_wasmi_with(true);
        let mut m = rt.object("Mover", &[]).unwrap();
        rt.debugger().set_breakpoint(5);
        let p = paused(
            rt.call_method_steppable(&mut m, "walk", &[Value::Int(2)])
                .unwrap(),
        );
        assert_eq!((p.line, p.reason), (5, StopReason::Breakpoint));
        let p = paused(rt.resume(Command::StepOver, &mut m).unwrap());
        assert_eq!((p.line, p.reason), (6, StopReason::Step));
        rt.debugger().clear_breakpoint(5);
        let done = rt.resume(Command::Continue, &mut m).unwrap();
        assert_eq!(done, Step::Done(Value::Float(3.0)));
        assert_eq!(rt.get(&m, "steps"), Some(Value::Int(2)));
    }

    #[test]
    fn an_asked_pause_stops_at_the_next_line() {
        let mut rt = on_wasmi();
        let mut m = rt.object("Mover", &[]).unwrap();
        rt.debugger().pause();
        let p = paused(
            rt.call_method_steppable(&mut m, "walk", &[Value::Int(1)])
                .unwrap(),
        );
        assert_eq!(p.reason, StopReason::Asked);
        let done = rt.resume(Command::Continue, &mut m).unwrap();
        assert_eq!(done, Step::Done(Value::Float(0.5)));
    }

    const GUARD: &str = "extern async fn wait(seconds: float);
pub struct Guard { hp: int }
impl Guard {
    pub async fn patrol(rounds: int) -> int {
        for r in 0..rounds {
            await wait(0.5);
            self.hp += r + 1;
        }
        self.hp
    }
}
";

    #[test]
    fn a_stop_inside_an_async_method_parks_its_poll() {
        let options = Options {
            debug: true,
            ..Options::default()
        };
        let c = compile(GUARD, &options).unwrap();
        let host = HostFns::default().on_async("wait", |args| {
            let Value::Float(s) = args[0] else {
                panic!("wait takes seconds")
            };
            (s, Value::Void)
        });
        let limits = Limits {
            engine: EngineKind::Wasmi,
            ..Limits::default()
        };
        let mut rt = Runtime::new(&c, host, limits).unwrap();
        let mut g = rt.object("Guard", &[]).unwrap();
        rt.debugger().set_breakpoint(7);
        let mut task = rt.start("Guard.patrol", &[Value::Int(2)]).unwrap();
        assert_eq!(
            rt.poll_steppable(&mut task, Some(&mut g)).unwrap(),
            Step::Waiting
        );
        rt.advance(1.0);
        let p = paused(rt.poll_steppable(&mut task, Some(&mut g)).unwrap());
        assert_eq!((p.line, p.frames[0].func.as_str()), (7, "Guard.patrol"));
        assert_eq!(local(&p, 0, "r"), Value::Int(0));
        assert!(
            rt.resume(Command::Continue, &mut g).is_err(),
            "not a method"
        );

        let p = paused(
            rt.resume_task(Command::StepOver, &mut task, Some(&mut g))
                .unwrap(),
        );
        assert_eq!((p.line, p.reason), (5, StopReason::Step));
        let done = rt.resume_task(Command::Continue, &mut task, Some(&mut g));
        assert_eq!(done.unwrap(), Step::Waiting);
        rt.advance(1.0);
        let p = paused(rt.poll_steppable(&mut task, Some(&mut g)).unwrap());
        assert_eq!(local(&p, 0, "r"), Value::Int(1));
        rt.debugger().clear_breakpoint(7);
        let done = rt.resume_task(Command::Continue, &mut task, Some(&mut g));
        assert_eq!(done.unwrap(), Step::Done(Value::Int(3)));
        assert_eq!(rt.get(&g, "hp"), Some(Value::Int(3)));
        assert_eq!(rt.live(), 0);
    }
}
