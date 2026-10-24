use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

const V1: &str = r"
    pub struct Spinner { angle: float, speed: float, name: str, history: [float] }
    impl Spinner {
        pub fn update(dt: float) {
            self.angle += self.speed * dt;
            self.history.push(self.angle);
        }
    }
";

const V2: &str = r#"
    pub struct Spinner { angle: float, speed: float, name: str, history: [float], turns: int }
    impl Spinner {
        pub fn update(dt: float) {
            self.angle += self.speed * 2 * dt;
            if self.angle > 6.0 { self.angle -= 6.0; self.turns += 1; }
            self.name = "{self.name}+";
        }
    }
"#;

#[test]
fn an_edit_keeps_every_field_that_kept_its_name() {
    let v1 = compile(V1, &Options::default()).unwrap();
    let mut rt = Runtime::new(&v1, HostFns::default(), Limits::default()).unwrap();
    let mut s = rt
        .object(
            "Spinner",
            &[
                ("speed", Value::Float(3.0)),
                ("name", Value::Str("top".into())),
            ],
        )
        .unwrap();
    for _ in 0..4 {
        rt.call_method(&mut s, "update", &[Value::Float(0.5)])
            .unwrap();
    }
    assert_eq!(rt.get(&s, "angle"), Some(Value::Float(6.0)));

    let v2 = compile(V2, &Options::default()).unwrap();
    let kept = rt.reload(&v2, HostFns::default(), &mut [&mut s]).unwrap();
    assert_eq!(kept[0], ["angle", "speed", "name", "history"]);
    rt.call_method(&mut s, "update", &[Value::Float(0.5)])
        .unwrap();
    assert_eq!(rt.get(&s, "angle"), Some(Value::Float(3.0)));
    assert_eq!(rt.get(&s, "turns"), Some(Value::Int(1)));
    assert_eq!(rt.get(&s, "name"), Some(Value::Str("top+".into())));
    assert_eq!(
        rt.get(&s, "history"),
        Some(Value::Array(
            [1.5, 3.0, 4.5, 6.0].map(Value::Float).to_vec()
        ))
    );
    rt.drop_object(s);
    assert_eq!(rt.live(), 0);
}
