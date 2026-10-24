// What an editor asks of a file: completion at the cursor, hover, go-to.

use balaur_bs::analysis::{Kind, analyze};
use balaur_bs::{HostApi, HostSignature, Literal, Ty};

const SRC: &str = "pub struct Mover {
    pub speed: float = 2.0,
    node: Node,
    hits: [str],
}

impl Mover {
    fn update(dt: float) {
        let reply: any = nil();
        let label = \"hi\";
        for h in self.hits {
            log(h);
        }
        self.speed += dt;
    }

    pub fn boost(by: float) -> float {
        self.speed * by
    }
}

fn log(text: str) {}
";

fn api() -> HostApi {
    let f = |key: &str, params: Vec<Ty>, ret: Ty| HostSignature {
        key: key.into(),
        params,
        ret,
        is_async: false,
    };
    HostApi {
        functions: vec![
            f("input::action_value", vec![Ty::Str], Ty::Float),
            f("input::action_down", vec![Ty::Str], Ty::Bool),
            f("node.get_node", vec![Ty::Node, Ty::Str], Ty::Node),
            f(
                "node.transform.translate",
                vec![Ty::Node, Ty::Float, Ty::Float, Ty::Float],
                Ty::Void,
            ),
        ],
        components: vec![(
            "transform".into(),
            vec![("position".into(), Ty::Struct("Vec3".into()))],
        )],
        constants: vec![("input::KEY_SPACE".into(), Literal::Str("space".into()))],
        structs: vec![(
            "ui::Rect".into(),
            vec![("x".into(), Ty::Float), ("width".into(), Ty::Float)],
        )],
    }
}

/// `src` with `typed` put in at the start of line 14, completed at its end.
fn complete_typing(typed: &str) -> Vec<String> {
    let lines: Vec<&str> = SRC.lines().collect();
    let mut src = lines[..13].join("\n");
    src.push('\n');
    src.push_str("        ");
    src.push_str(typed);
    src.push('\n');
    src.push_str(&lines[13..].join("\n"));
    let col = 9 + typed.chars().count() as u32;
    analyze(&src, &api())
        .complete(14, col)
        .into_iter()
        .map(|c| c.label)
        .collect()
}

#[test]
fn completion_follows_the_type_before_the_dot() {
    let fields = complete_typing("self.");
    assert!(
        fields.contains(&"speed".to_string()) && fields.contains(&"boost".to_string()),
        "{fields:?}"
    );
    assert_eq!(complete_typing("self.sp"), ["speed"]);
    let node = complete_typing("self.node.");
    assert!(
        node.contains(&"get_node".to_string()) && node.contains(&"transform".to_string()),
        "{node:?}"
    );
    assert!(
        !node.contains(&"translate".to_string()),
        "a component's op belongs on the component"
    );
    assert_eq!(
        complete_typing("self.node.transform."),
        ["position", "translate"]
    );
    assert!(complete_typing("reply.").contains(&"as_int".to_string()));
    assert!(complete_typing("label.").contains(&"to_upper".to_string()));
    assert!(complete_typing("self.hits.").contains(&"push".to_string()));
    assert_eq!(complete_typing("input::action_v"), ["action_value"]);
    assert_eq!(complete_typing("input::KEY"), ["KEY_SPACE"]);
    assert_eq!(complete_typing("ui::Re"), ["Rect"]);
    assert_eq!(
        complete_typing("let r = ui::Rect { x: 1.0, width: 2.0 }; r."),
        ["width", "x"]
    );
}

#[test]
fn a_bare_name_completes_locals_functions_and_modules() {
    let names = complete_typing("l");
    for want in ["label", "log", "let", "loop"] {
        assert!(
            names.contains(&want.to_string()),
            "{want} missing from {names:?}"
        );
    }
    assert!(complete_typing("in").contains(&"input".to_string()));
}

#[test]
fn hover_names_the_type_and_definition_finds_the_declaration() {
    let a = analyze(SRC, &api());
    assert!(a.error().is_none(), "{:?}", a.error());
    // `self.speed` on line 14, the `speed` part.
    assert_eq!(a.hover(14, 15).as_deref(), Some("speed: float"));
    assert_eq!(a.hover(12, 17).as_deref(), Some("h: str"));
    assert_eq!(a.hover(18, 15).as_deref(), Some("speed: float"));
    let log = a.definition(12, 13).expect("log is declared in the file");
    assert_eq!(log.line, 22);
    let speed = a.definition(14, 15).expect("speed is a field");
    assert_eq!((speed.line, speed.col), (2, 9));
    let label = a.definition(10, 13).expect("a local");
    assert_eq!(label.line, 10);
}

#[test]
fn the_error_is_where_the_compiler_stopped() {
    let a = analyze("fn f() -> int { true }", &api());
    let e = a.error().expect("a type error");
    assert_eq!(e.pos.line, 1);
    assert!(matches!(Kind::Field, Kind::Field));
}

#[test]
fn symbols_list_the_hooks_as_functions_and_the_tunables_as_properties() {
    let found = analyze(SRC, &api()).symbols();
    let named: Vec<(&str, Kind)> = found.iter().map(|s| (s.name.as_str(), s.kind)).collect();
    assert_eq!(
        named,
        [
            ("Mover", Kind::Struct),
            ("speed", Kind::Property),
            ("update", Kind::Function),
            ("boost", Kind::Function),
            ("log", Kind::Function),
        ]
    );
    assert_eq!(found[2].detail, "fn update(float)");
}

#[test]
fn signature_help_names_the_call_and_the_argument() {
    let src = SRC.replace("log(h);", "self.node.transform.translate(1.0, ");
    let a = analyze(&src, &api());
    let (sig, active) = a.signature(12, 49).expect("inside a call");
    assert_eq!(sig, "fn translate(float, float, float)");
    assert_eq!(active, 1);
    let src = SRC.replace("log(h);", "input::action_value(");
    let (sig, active) = analyze(&src, &api())
        .signature(12, 33)
        .expect("an engine call");
    assert_eq!(
        (sig.as_str(), active),
        ("fn input::action_value(str) -> float", 0)
    );
}

#[test]
fn rename_reaches_every_use_and_leaves_text_alone() {
    let src = "pub struct S { hp: int }\nimpl S {\n    fn update(dt: float) {\n        let msg = \"hp {self.hp}\";\n        self.hp += 1; // hp up\n    }\n}\n";
    let a = analyze(src, &api());
    assert_eq!(
        a.references("hp").len(),
        3,
        "the field, the interpolation and the add"
    );
    let renamed = a.rename("hp", "health").expect("a valid rename");
    assert!(
        renamed.contains("health: int")
            && renamed.contains("{self.health}")
            && renamed.contains("self.health += 1")
    );
    assert!(
        renamed.contains("\"hp ") && renamed.contains("// hp up"),
        "{renamed}"
    );
    assert!(a.rename("hp", "fn").is_none() && a.rename("hp", "2x").is_none());
}

#[test]
fn format_reindents_and_keeps_comments() {
    let messy = "pub struct S { hp: int }\nimpl S {\n  fn update(dt: float) {   \n      // grow\n self.hp += 1;\n\n\n\n        let v = vec3(1.0,\n2.0, 3.0);\n  }\n}\n\n\n";
    let want = "pub struct S { hp: int }\nimpl S {\n    fn update(dt: float) {\n        // grow\n        self.hp += 1;\n\n        let v = vec3(1.0,\n            2.0, 3.0);\n    }\n}\n";
    let a = analyze(messy, &api());
    assert_eq!(a.format(), want);
    assert_eq!(
        analyze(want, &api()).format(),
        want,
        "formatting is idempotent"
    );
}

#[test]
fn an_optional_offers_its_four_methods() {
    let src = "fn f(x: str?) -> int {\n    x.\n    1\n}\n";
    let got: Vec<String> = analyze(src, &HostApi::default())
        .complete(2, 7)
        .into_iter()
        .map(|c| format!("{}: {}", c.label, c.detail))
        .collect();
    for want in [
        "is_some: fn is_some() -> bool",
        "unwrap: fn unwrap() -> str",
        "or: fn or(str) -> str",
    ] {
        assert!(got.contains(&want.to_string()), "{got:?}");
    }
}

#[test]
fn a_used_module_offers_its_public_names() {
    use balaur_bs::Modules;
    use std::rc::Rc;
    let util =
        "pub struct Rect { w: float }\npub fn area(r: Rect) -> float { r.w }\nfn hidden() {}\n";
    let modules = Modules(Rc::new(move |_from: &str, name: &str| {
        (name == "util").then(|| ("util.bs".to_string(), util.to_string()))
    }));
    let src = "use util;\nfn f() -> float {\n    util::\n    1.0\n}\n";
    let a =
        balaur_bs::analysis::analyze_in(src, &HostApi::default(), Some(modules.clone()), "main.bs");
    let got: Vec<String> = a.complete(3, 11).into_iter().map(|c| c.label).collect();
    assert!(
        got.contains(&"area".into()) && got.contains(&"Rect".into()),
        "{got:?}"
    );
    assert!(!got.iter().any(|g| g.contains("hidden")), "{got:?}");
    let src = "use util;\nfn f(r: util::Rect) -> float { util::area(r) }\n";
    let a = balaur_bs::analysis::analyze_in(src, &HostApi::default(), Some(modules), "main.bs");
    assert!(a.error().is_none(), "{:?}", a.error());
    assert_eq!(
        a.hover(2, 39).as_deref(),
        Some("fn util::area(util::Rect) -> float")
    );
}
