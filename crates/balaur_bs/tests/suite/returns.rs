use super::run_with;
use balaur_bs::runtime::{HostFns, Value};
use balaur_bs::{HostApi, HostSignature, Options, Ty};

const SCRIPT: &str = "
extern fn greeting(name: str) -> str;

pub fn joined() -> str {
    let all = \"\";
    for n in world::names() {
        all = \"{all}{n},\";
    }
    all
}

pub fn greet() -> str {
    greeting(\"Ana\")
}

pub fn count() -> int {
    world::names().len()
}
";

fn api() -> HostApi {
    HostApi {
        functions: vec![HostSignature {
            key: "world::names".into(),
            params: vec![],
            ret: Ty::Array(Box::new(Ty::Str)),
            is_async: false,
        }],
        components: vec![],
        constants: vec![],
        structs: vec![],
    }
}

fn host() -> HostFns {
    HostFns::default()
        .on("greeting", |args| match &args[0] {
            Value::Str(n) => Value::Str(format!("hello {n}")),
            _ => Value::Void,
        })
        .engine(|_, _, _| {
            Ok(Value::Array(vec![
                Value::Str("Ana".into()),
                Value::Str("Bo".into()),
            ]))
        })
}

#[test]
fn the_host_hands_back_strings_and_arrays_the_script_owns() {
    let options = Options {
        host: api(),
        ..Options::default()
    };
    let mut rt = run_with(SCRIPT, host(), &options);
    let base = rt.live();
    assert_eq!(
        rt.call("joined", &[]).unwrap(),
        Value::Str("Ana,Bo,".into())
    );
    assert_eq!(
        rt.call("greet", &[]).unwrap(),
        Value::Str("hello Ana".into())
    );
    assert_eq!(rt.call("count", &[]).unwrap(), Value::Int(2));
    assert_eq!(rt.live(), base, "a returned string or array leaked");
}
