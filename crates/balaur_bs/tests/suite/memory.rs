use super::{call, run};
use balaur_bs::runtime::Value;

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
