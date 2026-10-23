//! The translator's tests for inner classes: a script of their own, what
//! they see of the outer class, and how a preload reaches one.

use super::convert;
use super::tests::{assert_compiles, flat};
use crate::godot::exports::Classes;

#[test]
fn an_inner_class_is_a_script_of_its_own() {
    let source = "extends Node\n\
class Tracker:\n\
\tvar seen := 0\n\
\tfunc bump():\n\
\t\tseen += 1\n\
\n\
func make():\n\
\treturn Tracker.new()\n";
    let inner = super::inner_classes(source);
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].0, "Tracker");
    assert!(
        inner[0]
            .1
            .starts_with("extends RefCounted\nvar seen := 0\nfunc bump():\n\tseen += 1\n"),
        "{}",
        inner[0].1
    );
    let out = convert(source, "scripts/r.gd", &Classes::default());
    assert!(
        out.text.contains("\nuse scripts::r__Tracker;\n"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("return r__Tracker::new();"),
        "{}",
        out.text
    );
    // The inner class is an object class of its own, and both compile.
    let file = super::inner_file("scripts/r.gd", "Tracker");
    let (_, text) = &super::inner_scripts(source, "scripts/r.gd")[0];
    let tracker = convert(text, &file, &Classes::default());
    let tracker_path = crate::godot::scene::script_path(&file);
    assert!(
        flat(&tracker.text).contains(
            r#"pub fn bump(this: any) { this.set("seen", gd::add(this.get("seen"), 1)); }"#
        ),
        "{}",
        tracker.text
    );
    assert_compiles(&file, &tracker.text, &[]);
    assert_compiles(
        "scripts/r.gd",
        &out.text,
        &[(tracker_path.as_str(), tracker.text.clone())],
    );
}

#[test]
fn an_inner_class_sees_the_outers_constants_and_its_siblings() {
    let source = "extends RefCounted\n\
enum PB_ERR { NO_ERRORS = 0, TRUNCATED = 1 }\n\
const LIMIT = 9\n\
class Field:\n\
\tvar state := 0\n\
class Packer:\n\
\tconst LIMIT = 3\n\
\tstatic func check(value):\n\
\t\tvar field = Field.new()\n\
\t\treturn value == PB_ERR.NO_ERRORS and field.state < LIMIT\n";
    let inners = super::inner_scripts(source, "proto/pb.gd");
    assert_eq!(inners.len(), 2);
    let packer = &inners[1].1;
    assert!(
        packer.contains("const Field = preload(\"res://proto/pb__Field.gd\")"),
        "{packer}"
    );
    assert!(
        packer.contains("enum PB_ERR { NO_ERRORS = 0, TRUNCATED = 1 }"),
        "{packer}"
    );
    assert_eq!(packer.matches("const LIMIT").count(), 1, "{packer}");
    let out = convert(packer, "proto/pb__Packer.gd", &Classes::default());
    for want in [
        "pub const PB_ERR__NO_ERRORS: int = 0;",
        "pub const PB_ERR__TRUNCATED: int = 1;",
        "pub fn PB_ERR() -> any {",
        "pub const LIMIT: int = 3;",
        "\nuse proto::pb__Field;\n",
        "let field: any = pb__Field::new();",
        "gd::same(value, PB_ERR__NO_ERRORS)",
        r#"gd::lt(gd::field(field, "state"), LIMIT)"#,
    ] {
        assert!(out.text.contains(want), "no `{want}` in:\n{}", out.text);
    }
    assert!(!out.text.contains("PORT("), "{}", out.text);
}

#[test]
fn an_inner_class_is_reached_through_a_preload_of_its_file() {
    let mut classes = Classes::default();
    classes.inner.insert(
        "proto/pb.bs.Field".to_string(),
        "proto/pb__Field.gd".to_string(),
    );
    let source = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Field.new()\n";
    let out = convert(source, "scripts/u.gd", &classes);
    assert!(
        out.text.contains("\nuse proto::pb__Field;\n"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("return pb__Field::new();"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("gd::call("), "{}", out.text);
}

#[test]
fn a_class_inside_an_inner_class_is_reached_from_inside_and_outside() {
    let source = "extends RefCounted\n\
class Msg:\n\
\tclass Part:\n\
\t\tvar n := 0\n\
\tfunc make():\n\
\t\treturn Msg.Part.new()\n";
    let inners = super::inner_scripts(source, "proto/pb.gd");
    let msg = &inners[0].1;
    assert!(
        msg.starts_with("extends RefCounted\nclass_name Msg\n"),
        "{msg}"
    );
    let nested = super::inner_scripts(msg, "proto/pb__Msg.gd");
    assert_eq!(nested[0].0, "Part");
    let mut classes = Classes::default();
    classes
        .inner
        .insert("proto/pb.bs.Msg".into(), "proto/pb__Msg.gd".into());
    classes.inner.insert(
        "proto/pb__Msg.bs.Part".into(),
        "proto/pb__Msg__Part.gd".into(),
    );
    let part = "return pb__Msg__Part::new();";
    let out = convert(msg, "proto/pb__Msg.gd", &classes);
    assert!(out.text.contains(part), "{}", out.text);
    assert!(
        out.text.contains("\nuse proto::pb__Msg__Part;\n"),
        "{}",
        out.text
    );
    let user = "extends Node\n\
const PB = preload(\"res://proto/pb.gd\")\n\
func make():\n\
\treturn PB.Msg.Part.new()\n";
    let out = convert(user, "scripts/u.gd", &classes);
    assert!(out.text.contains(part), "{}", out.text);
    assert!(
        out.text.contains("\nuse proto::pb__Msg__Part;\n"),
        "{}",
        out.text
    );
}
