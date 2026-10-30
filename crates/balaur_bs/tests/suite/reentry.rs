use balaur_bs::runtime::{EngineKind, HostFns, Limits, Object, Reentry, Runtime, Value};
use balaur_bs::{HostApi, HostSignature, Options, Ty, compile};
use std::cell::RefCell;
use std::rc::Rc;

const SCRIPT: &str = "pub class Counter { n: int, name: str }
impl Counter {
    pub fn poke_other(k: int) -> int {
        let got = other::poke(k);
        got + self.n
    }
    pub fn add(k: int) -> int {
        self.n += k;
        self.name = \"poked {self.n}\";
        self.n
    }
    pub fn crash(k: int) -> int {
        let a = [1];
        a[k]
    }
}
";

/// What the engine reaches back into: the file's way in, and a second
/// object of it, as a node running the same script would be.
#[derive(Default)]
struct Other {
    way: Option<Reentry>,
    obj: Option<Object>,
    method: &'static str,
}

fn api() -> HostApi {
    HostApi {
        functions: vec![HostSignature {
            key: "other::poke".into(),
            params: vec![Ty::Int],
            ret: Ty::Int,
            is_async: false,
        }],
        components: vec![],
        constants: vec![],
        structs: vec![],
    }
}

fn runtime(engine: EngineKind, debug: bool) -> (Runtime, Rc<RefCell<Other>>, Object) {
    let other = Rc::new(RefCell::new(Other {
        method: "add",
        ..Other::default()
    }));
    let reach = other.clone();
    let host = HostFns::default().engine(move |_, args, _| {
        let (way, obj, method) = {
            let mut o = reach.borrow_mut();
            (o.way.clone(), o.obj.take(), o.method)
        };
        let (Some(way), Some(mut obj)) = (way, obj) else {
            return Err("nothing to reach".into());
        };
        let out = way.call_method(&mut obj, method, args);
        reach.borrow_mut().obj = Some(obj);
        out.map_err(|e| e.to_string())
    });
    let options = Options {
        host: api(),
        debug,
        ..Options::default()
    };
    let c = compile(SCRIPT, &options).unwrap();
    let limits = Limits {
        engine,
        ..Limits::default()
    };
    let mut rt = Runtime::new(&c, host, limits).unwrap();
    let me = rt.object("Counter", &[]).unwrap();
    let theirs = rt.object("Counter", &[]).unwrap();
    {
        let mut o = other.borrow_mut();
        o.way = Some(rt.reentry());
        o.obj = Some(theirs);
    }
    (rt, other, me)
}

fn calls_back_on(engine: EngineKind) {
    let (mut rt, other, mut me) = runtime(engine, false);
    let base = rt.live();
    let poke = |rt: &mut Runtime, me: &mut Object, k| {
        rt.call_method(me, "poke_other", &[Value::Int(k)]).unwrap()
    };
    assert_eq!(poke(&mut rt, &mut me, 3), Value::Int(3));
    assert_eq!(poke(&mut rt, &mut me, 4), Value::Int(7));
    let theirs = other.borrow_mut().obj.take().unwrap();
    assert_eq!(rt.get(&theirs, "name"), Some(Value::Str("poked 7".into())));
    assert_eq!(rt.live(), base, "the call back in leaked");

    // A trap in the inner call fails the outer one, and the file runs on.
    other.borrow_mut().obj = Some(theirs);
    other.borrow_mut().method = "crash";
    let e = rt
        .call_method(&mut me, "poke_other", &[Value::Int(5)])
        .unwrap_err();
    assert!(e.msg.contains("unreachable"), "{e}");
    other.borrow_mut().method = "add";
    assert_eq!(poke(&mut rt, &mut me, 1), Value::Int(8));
}

#[test]
fn a_script_reaches_its_own_file_through_the_engine() {
    calls_back_on(EngineKind::Auto);
}

#[cfg(feature = "wasmi")]
#[test]
fn a_script_reaches_its_own_file_on_wasmi() {
    calls_back_on(EngineKind::Wasmi);
}

// The outer call parked at a stop after the inner one ran; a breakpoint in
// the inner call is passed by, since only the outermost call parks.
#[cfg(feature = "wasmi")]
#[test]
fn a_parked_call_holds_across_a_call_back_in() {
    use balaur_bs::runtime::{Command, Step};
    let (mut rt, _other, mut me) = runtime(EngineKind::Wasmi, true);
    rt.debugger().set_breakpoint(5);
    rt.debugger().set_breakpoint(8);
    let step = rt
        .call_method_steppable(&mut me, "poke_other", &[Value::Int(2)])
        .unwrap();
    let Step::Paused(p) = step else {
        panic!("{step:?} instead of stopping")
    };
    assert_eq!((p.line, p.frames.len()), (5, 1));
    let done = rt.resume(Command::Continue, &mut me).unwrap();
    assert_eq!(done, Step::Done(Value::Int(2)));
}
