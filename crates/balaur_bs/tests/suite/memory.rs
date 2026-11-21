use super::{call, run};
use balaur_bs::runtime::{EngineKind, HostFns, Limits, Runtime, Value};
use balaur_bs::{HostApi, HostSignature, Options, Ty, compile};
use std::cell::RefCell;
use std::rc::Rc;

const CHURN: &str = r#"
    struct Item { name: str, tags: [str] }

    pub fn strings(n: int) -> int {
        let s = "";
        for i in 0..n { s = s + "{i},"; }
        s.len()
    }

    pub fn nested(n: int) -> int {
        let items: [Item] = [];
        for i in 0..n {
            let tags = ["a", "b"];
            tags.push("tag {i}");
            items.push(Item { name: "item {i}", tags: tags });
        }
        let kept = items[n - 1];
        items = [];
        kept.tags.len()
    }

    pub fn early(n: int) -> str {
        let words = ["x", "y"];
        for i in 0..n {
            let w = "w{i}";
            if i == 3 { return w; }
        }
        words[0]
    }

    pub fn shared() -> int {
        let a = ["one", "two"];
        let b = a;
        b.push("three");
        a.len()
    }
"#;

#[test]
fn freed_memory_is_reused() {
    let mut rt = run(CHURN);
    call(&mut rt, "strings", &[Value::Int(2000)]);
    let after_one = rt.memory_bytes();
    for _ in 0..20 {
        call(&mut rt, "strings", &[Value::Int(2000)]);
    }
    assert_eq!(rt.memory_bytes(), after_one);
    assert_eq!(rt.live(), 0);
}

#[test]
fn host_objects_own_their_fields() {
    let mut rt = run(r#"
        pub class Bag { items: [str], label: str }
        impl Bag {
            pub fn add(what: str) { self.items.push(what); self.label = "{self.items.len()} items"; }
        }
    "#);
    let base = rt.live();
    let bag = rt.object("Bag", &[]).unwrap();
    for w in ["a", "b", "c"] {
        rt.call_method(&bag, "add", &[Value::Str(w.into())])
            .unwrap();
    }
    assert_eq!(rt.get(&bag, "label"), Some(Value::Str("3 items".into())));
    assert_eq!(
        rt.get(&bag, "items"),
        Some(Value::Array(vec![
            Value::Str("a".into()),
            Value::Str("b".into()),
            Value::Str("c".into())
        ]))
    );
    rt.drop_object(bag);
    assert_eq!(rt.live(), base);
}

const REWRITES: &str = r#"
    pub class Screen { node: Node, frame: int }
    impl Screen {
        fn fill(rows: [ui::Row]) { self.node.list.rows = rows; }

        pub fn update() {
            self.frame += 1;
            let rows: [ui::Row] = [];
            for i in 0..4 {
                rows.push(ui::Row { label: "row {i} of frame {self.frame}", cells: ["a{i}", "b{i}"] });
            }
            self.fill(rows);
            self.node.list.caption = "frame {self.frame}";
            self.node.list.caption += "!";
            self.node.list.picked = ["row {self.frame}"];
            self.node.list.extra = { "frame": self.frame };
            self.node.list.first = ui::Row { label: "first of {self.frame}", cells: [] };
            draw::row(ui::Row { label: "drawn in {self.frame}", cells: ["c"] });
            draw::caption("caption {self.frame}");
        }
    }
"#;

fn rewriting() -> Options {
    let list = |of: Ty| Ty::Array(Box::new(of));
    let row = Ty::Struct("ui::Row".into());
    let takes = |key: &str, param: Ty| HostSignature {
        key: key.into(),
        params: vec![param],
        ret: Ty::Void,
        is_async: false,
        editor: false,
    };
    Options {
        host: HostApi {
            functions: vec![
                takes("draw::row", row.clone()),
                takes("draw::caption", Ty::Optional(Box::new(Ty::Str))),
            ],
            components: vec![(
                "list".into(),
                vec![
                    ("rows".into(), list(row.clone())),
                    ("caption".into(), Ty::Str),
                    ("picked".into(), list(Ty::Str)),
                    ("extra".into(), Ty::Any),
                    ("first".into(), row),
                ],
            )],
            structs: vec![(
                "ui::Row".into(),
                vec![("label".into(), Ty::Str), ("cells".into(), list(Ty::Str))],
            )],
            ..HostApi::default()
        },
        ..Options::default()
    }
}

const FRAMES: usize = 500;

// The engine copies what a script hands it, so each value is the script's to free.
#[test]
fn a_value_handed_to_the_engine_every_frame_is_freed_every_frame() {
    let compiled = compile(REWRITES, &rewriting()).unwrap_or_else(|error| panic!("{error}"));
    let handed = Rc::new(RefCell::new(vec![]));
    let seen = handed.clone();
    let host = HostFns::default().engine(move |key, args, _| {
        if key == "list.caption.get" {
            return Ok(Value::Str("read".into()));
        }
        seen.borrow_mut().push((key.to_string(), args.to_vec()));
        Ok(Value::Void)
    });
    let mut rt = Runtime::new(&compiled, host, Limits::default()).unwrap();
    let screen = rt.object("Screen", &[("node", Value::Node(7))]).unwrap();
    let held = rt.live();
    rt.call_method(&screen, "update", &[]).unwrap();
    let (each, bytes) = (handed.borrow().len(), rt.memory_bytes());
    assert!(each > 0, "the engine was handed nothing");
    assert_eq!(rt.live(), held, "the first frame kept what it handed over");
    for _ in 1..FRAMES {
        rt.call_method(&screen, "update", &[]).unwrap();
    }
    assert_eq!(rt.live(), held, "a frame kept what it handed over");
    assert_eq!(rt.memory_bytes(), bytes);
    let handed = handed.borrow();
    assert_eq!(handed.len(), each * FRAMES);
    let last = |wanted: &str| {
        let found = handed.iter().rev().find(|(key, _)| key == wanted);
        format!("{:?}", found.map(|(_, args)| args.last()))
    };
    let rows = last("list.rows.set");
    assert!(rows.contains(&format!("row 3 of frame {FRAMES}")), "{rows}");
    assert!(last("list.caption.set").contains("read!"));
    assert!(last("draw::caption").contains(&format!("caption {FRAMES}")));
    rt.drop_object(screen);
    assert_eq!(rt.live(), 0);
}

const DOCS: &str = r#"
    extern fn doc(rows: int) -> any;

    pub fn read(times: int, rows: int) -> int {
        let seen = 0;
        for t in 0..times {
            let d = doc(rows);
            for row in d["rows"] {
                seen += row["tags"].len() + row["name"].as_str().len();
            }
        }
        seen
    }

    pub fn own(n: int) -> int {
        let mine: [str] = [];
        for i in 0..n { mine.push("mine {i}"); }
        mine.len()
    }
"#;

// A document of `rows` rows, each a name and a list of tags, as a parsed
// file hands a script one: a block a value.
fn doc(rows: i64) -> Value {
    let row = |i: i64| {
        Value::Map(vec![
            ("name".into(), Value::Str(format!("row {i}"))),
            (
                "tags".into(),
                Value::Array(vec![Value::Str("a".into()), Value::Str(format!("tag {i}"))]),
            ),
        ])
    };
    Value::Map(vec![(
        "rows".into(),
        Value::Array((0..rows).map(row).collect()),
    )])
}

// Blocks the engine takes as `__alloc` would (`runtime::heap`) are freed,
// reused and laid out as the script's own, on every engine.
#[test]
fn what_the_engine_hands_a_script_is_freed_and_reused_alike_on_every_engine() {
    let run_on = |engine| {
        let compiled = compile(DOCS, &Options::default()).unwrap_or_else(|e| panic!("{e}"));
        let host = HostFns::default().on("doc", |args| match args.first() {
            Some(Value::Int(rows)) => doc(*rows),
            _ => Value::Void,
        });
        let limits = Limits {
            engine,
            ..Limits::default()
        };
        let mut rt = Runtime::new(&compiled, host, limits).unwrap_or_else(|e| panic!("{e}"));
        let once = call(&mut rt, "read", &[Value::Int(1), Value::Int(400)]);
        assert_eq!(rt.live(), 0, "a document outlived the call that read it");
        // A loop holds its last document while the next one comes, so two
        // stand at once from the second turn on: the heap is that big after.
        call(&mut rt, "read", &[Value::Int(3), Value::Int(400)]);
        let bytes = rt.memory_bytes();
        let many = call(&mut rt, "read", &[Value::Int(30), Value::Int(400)]);
        assert_eq!(
            rt.memory_bytes(),
            bytes,
            "a later document did not reuse the first's blocks"
        );
        let own = call(&mut rt, "own", &[Value::Int(400)]);
        assert_eq!(
            rt.memory_bytes(),
            bytes,
            "the script's own lists did not reuse them"
        );
        assert_eq!(rt.live(), 0);
        (once, many, own, bytes)
    };
    let wasmi = run_on(EngineKind::Wasmi);
    // Two tags a row, and each name's letters: "row " and its number.
    let names: usize = (0..400).map(|i: usize| i.to_string().len() + 4).sum();
    assert_eq!(wasmi.0, Value::Int(i64::try_from(400 * 2 + names).unwrap()));
    // `jit`, not the feature: the feature asks, and iOS refuses a JIT.
    #[cfg(jit)]
    assert_eq!(run_on(EngineKind::Wasmtime), wasmi);
}
