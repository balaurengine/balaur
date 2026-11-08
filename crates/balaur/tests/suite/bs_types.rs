//! `types::KIND_*` and what `kind()` answers on an `any`: the prelude spells
//! one and the host the other, so only a running script can hold them together.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

const KINDS: &str = "pub class Kinds { node: Node, matched: int }

fn counted(value: any, kind: str) -> int {
    if value.kind() == kind { 1 } else { 0 }
}

impl Kinds {
    fn init() {
        self.matched = counted(nil, types::KIND_NIL) +
            counted(true, types::KIND_BOOL) +
            counted(1, types::KIND_INT) +
            counted(1.5, types::KIND_FLOAT) +
            counted(\"a\", types::KIND_STRING) +
            counted([1], types::KIND_LIST) +
            counted({ \"a\": 1 }, types::KIND_MAP) +
            counted(self.node, types::KIND_NODE);
    }
}
";

#[test]
fn every_kind_an_any_answers_has_its_types_constant() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"kinds\"\nname = \"Kinds\"\nscript = { source = \"scripts/kinds.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/kinds.bs"), KINDS).unwrap();
    let app = super::bs::booted(dir.path());
    let node = {
        let world = app.engine.world();
        balaur_core::scene::find_node(&world, app.engine.root(), "Kinds").unwrap()
    };
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.script_field(balaur_core::node_id_of(node), "matched"),
        Some(balaur_script::Value::Int(8))
    );
}
