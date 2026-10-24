use super::run_with;
use balaur_bs::runtime::{HostFns, Runtime, Task, Value};
use balaur_bs::{HostSignature, Options, Ty};
use std::cell::RefCell;
use std::rc::Rc;

const SCRIPT: &str = r#"
    extern async fn wait(seconds: float);
    extern async fn roll() -> int;
    extern fn say(text: str);

    pub struct Guard { name: str, hp: int, log: [str] }

    impl Guard {
        // Patrols `rounds` times, healing by whatever the host rolls each round.
        pub async fn patrol(rounds: int) {
            for r in 0..rounds {
                await wait(0.5);
                let heal = await roll();
                self.hp += heal;
                self.log.push("round {r}: +{heal}");
                say("{self.name} at {self.hp}");
            }
            let left = await self.rest(1.0);
            say("rested, {left} left");
        }

        async fn rest(seconds: float) -> int {
            await wait(seconds);
            self.hp += 1;
            100 - self.hp
        }
    }

    pub async fn countdown(from: int) -> str {
        let parts: [str] = [];
        let n = from;
        while n > 0 {
            parts.push("{n}");
            await wait(1.0);
            n -= 1;
        }
        "{parts.len()} ticks"
    }
"#;

fn host(said: Rc<RefCell<Vec<String>>>) -> HostFns {
    let mut next = 0;
    HostFns::default()
        .on_async("wait", |args| {
            let Value::Float(s) = args[0] else { panic!() };
            (s, Value::Void)
        })
        .on_async("roll", move |_| {
            next += 1;
            (0.25, Value::Int(next * 10))
        })
        .on("say", move |args| {
            if let Value::Str(s) = &args[0] {
                said.borrow_mut().push(s.clone());
            }
            Value::Void
        })
}

fn drive(
    rt: &mut Runtime,
    task: &mut Task,
    mut obj: Option<&mut balaur_bs::runtime::Object>,
) -> (Value, f64, usize) {
    for polls in 0..1000 {
        if let Some(v) = rt.poll(task, obj.as_deref_mut()).unwrap() {
            return (v, rt.now(), polls + 1);
        }
        rt.advance(1.0 / 60.0);
    }
    panic!("never finished");
}

#[test]
fn a_method_awaits_the_host_and_another_script_fn() {
    let said = Rc::new(RefCell::new(vec![]));
    let mut rt = run_with(SCRIPT, host(said.clone()), &Options::default());
    let base = rt.live();
    let mut guard = rt
        .object(
            "Guard",
            &[("name", Value::Str("Ana".into())), ("hp", Value::Int(50))],
        )
        .unwrap();
    let mut task = rt.start("Guard.patrol", &[Value::Int(2)]).unwrap();
    let (v, t, _) = drive(&mut rt, &mut task, Some(&mut guard));
    assert_eq!(v, Value::Void);
    assert!((t - 2.5).abs() < 0.05, "finished at {t}");
    assert_eq!(
        *said.borrow(),
        ["Ana at 60", "Ana at 80", "rested, 19 left"]
    );
    assert_eq!(rt.get(&guard, "hp"), Some(Value::Int(81)));
    assert_eq!(
        rt.get(&guard, "log"),
        Some(Value::Array(vec![
            Value::Str("round 0: +10".into()),
            Value::Str("round 1: +20".into())
        ]))
    );
    rt.drop_object(guard);
    assert_eq!(rt.live(), base, "a finished task leaked");
}

#[test]
fn a_free_async_fn_returns_a_value() {
    let mut rt = run_with(SCRIPT, host(Rc::default()), &Options::default());
    let mut task = rt.start("countdown", &[Value::Int(3)]).unwrap();
    let (v, t, _) = drive(&mut rt, &mut task, None);
    assert_eq!(v, Value::Str("3 ticks".into()));
    assert!((t - 3.0).abs() < 0.05);
    assert_eq!(rt.live(), 0);
}

#[test]
fn await_belongs_in_async_functions() {
    let e = super::compile_error("extern async fn wait(s: float);\nfn f() { await wait(1.0); }");
    assert!(e.contains("`await` goes in an `async fn`"), "{e}");
    let e = super::compile_error("extern async fn wait(s: float);\nasync fn f() { wait(1.0); }");
    assert!(e.contains("is async"), "{e}");
}

const ENGINE_WAITS: &str = "
    pub async fn fetch_after(seconds: float) -> int {
        await task::seconds(seconds);
        let got = await net::fetch();
        got + 1
    }
";

fn engine_api() -> balaur_bs::HostApi {
    let f = |key: &str, params: Vec<Ty>, ret: Ty| HostSignature {
        key: key.into(),
        params,
        ret,
        is_async: true,
    };
    balaur_bs::HostApi {
        functions: vec![
            f("task::seconds", vec![Ty::Float], Ty::Void),
            f("net::fetch", vec![], Ty::Int),
        ],
        components: vec![],
        constants: vec![],
        structs: vec![],
    }
}

#[test]
fn an_engine_await_waits_for_its_token() {
    let asked = Rc::new(RefCell::new(Vec::<String>::new()));
    let log = asked.clone();
    let next = std::cell::Cell::new(100);
    let host = HostFns::default().engine(move |key, _, _| {
        log.borrow_mut().push(key.to_string());
        next.set(next.get() + 1);
        Ok(Value::Int(next.get()))
    });
    let options = Options {
        host: engine_api(),
        ..Options::default()
    };
    let mut rt = run_with(ENGINE_WAITS, host, &options);
    let mut task = rt.start("fetch_after", &[Value::Float(1.0)]).unwrap();
    assert_eq!(rt.poll(&mut task, None).unwrap(), None);
    assert_eq!(*asked.borrow(), ["task::seconds"]);
    assert_eq!(rt.awaits(101), Some(Ty::Void));
    assert!(!rt.wake(7, Value::Void), "nothing awaits token 7");
    assert_eq!(rt.poll(&mut task, None).unwrap(), None, "woke too early");

    assert!(rt.wake(101, Value::Void));
    assert_eq!(rt.poll(&mut task, None).unwrap(), None);
    assert_eq!(*asked.borrow(), ["task::seconds", "net::fetch"]);
    assert!(rt.wake(102, Value::Int(41)));
    assert_eq!(rt.poll(&mut task, None).unwrap(), Some(Value::Int(42)));
    assert_eq!(rt.live(), 0);
}

#[test]
fn a_cancelled_task_never_resumes() {
    let host = HostFns::default().engine(|_, _, _| Ok(Value::Int(5)));
    let options = Options {
        host: engine_api(),
        ..Options::default()
    };
    let mut rt = run_with(ENGINE_WAITS, host, &options);
    let mut task = rt.start("fetch_after", &[Value::Float(1.0)]).unwrap();
    assert_eq!(rt.poll(&mut task, None).unwrap(), None);
    rt.cancel(task).unwrap();
    assert!(
        !rt.wake(5, Value::Void),
        "a cancelled task still awaited its token"
    );
}

const HEAP_AWAITS: &str = "
    extern async fn fetch(name: str) -> str;

    pub async fn greet() -> str {
        let a = await fetch(\"ana\");
        let reply = await net::reply();
        let tags = await net::tags();
        let hp = reply[\"hp\"].as_int();
        \"{a} {hp} {tags.len()}\"
    }
";

#[test]
fn an_await_hands_back_a_string_an_array_or_an_any() {
    let f = |key: &str, ret: Ty| HostSignature {
        key: key.into(),
        params: vec![],
        ret,
        is_async: true,
    };
    let api = balaur_bs::HostApi {
        functions: vec![
            f("net::reply", Ty::Any),
            f("net::tags", Ty::Array(Box::new(Ty::Str))),
        ],
        components: vec![],
        constants: vec![],
        structs: vec![],
    };
    let next = std::cell::Cell::new(10);
    let host = HostFns::default()
        .on_async("fetch", |args| match &args[0] {
            Value::Str(n) => (0.0, Value::Str(format!("hi {n}"))),
            _ => (0.0, Value::Void),
        })
        .engine(move |_, _, _| {
            next.set(next.get() + 1);
            Ok(Value::Int(next.get()))
        });
    let options = Options {
        host: api,
        ..Options::default()
    };
    let mut rt = run_with(HEAP_AWAITS, host, &options);
    let mut task = rt.start("greet", &[]).unwrap();
    rt.advance(0.1);
    assert_eq!(rt.poll(&mut task, None).unwrap(), None);
    rt.advance(0.1);
    assert_eq!(
        rt.poll(&mut task, None).unwrap(),
        None,
        "the reply has not come"
    );
    assert_eq!(rt.awaits(11), Some(Ty::Any));
    assert!(rt.wake(11, Value::Map(vec![("hp".into(), Value::Int(7))])));
    assert_eq!(rt.poll(&mut task, None).unwrap(), None);
    let tags = Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]);
    assert!(rt.wake(12, tags));
    assert_eq!(
        rt.poll(&mut task, None).unwrap(),
        Some(Value::Str("hi ana 7 2".into()))
    );
    assert_eq!(rt.live(), 0, "an awaited value leaked");
}

// An `await` in an `else if`, in an `if` nested last in a branch, and in an
// `if` that ends the function: each is a block's tail, and each runs.
const TAILS: &str = r#"
    extern async fn wait(seconds: float);
    extern fn say(text: str);
    pub async fn branches(n: int) {
        if n == 1 {
            await wait(0.1);
            say("one");
        } else if n == 2 {
            await wait(0.1);
            say("two");
        } else {
            await wait(0.1);
            say("other");
        }
        if n == 2 {
            if n == 2 {
                await wait(0.1);
                say("nested");
            }
        }
        while n == 2 {
            if n == 2 {
                await wait(0.1);
                say("in a loop");
                break;
            }
        }
        if n == 2 {
            await wait(0.1);
            say("last");
        }
    }
"#;

#[test]
fn an_await_in_a_block_s_tail_runs() {
    let said = Rc::new(RefCell::new(vec![]));
    let mut rt = run_with(TAILS, host(said.clone()), &Options::default());
    let mut task = rt.start("branches", &[Value::Int(2)]).unwrap();
    drive(&mut rt, &mut task, None);
    assert_eq!(*said.borrow(), ["two", "nested", "in a loop", "last"]);
}
