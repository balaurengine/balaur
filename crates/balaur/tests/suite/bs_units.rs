//! Units kept in the data folder between runs. Several processes share that
//! folder, so what a run finds there may be another's half-finished write.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use balaur_core::engine_api::user_data_dir_of;

const BOOTED: &str = "pub class Booted { node: Node, ran: int }

impl Booted {
    fn init() {
        self.ran = 1;
    }
}
";

fn ran(app: &balaur::App) -> Option<balaur_script::Value> {
    let node = {
        let world = app.engine.world();
        balaur_core::scene::find_node(&world, app.engine.root(), "Booted").unwrap()
    };
    let host = app.engine.script_host().unwrap();
    host.script_field(balaur_core::node_id_of(node), "ran")
}

#[test]
fn a_kept_unit_left_empty_is_built_again() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!(
            "[application]\nname = \"empty-unit-{}\"\nmain_scene = \"main.toml\"\n",
            std::process::id()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"booted\"\nname = \"Booted\"\nscript = { source = \"scripts/booted.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/booted.bs"), BOOTED).unwrap();

    let first = super::bs::booted(dir.path());
    assert_eq!(ran(&first), Some(balaur_script::Value::Int(1)));
    let data = user_data_dir_of(&first.engine);
    drop(first);

    let mut emptied = 0;
    for entry in std::fs::read_dir(data.join("bs-cache")).unwrap().flatten() {
        if entry.path().extension().is_some_and(|ext| ext == "unit") {
            std::fs::write(entry.path(), b"").unwrap();
            emptied += 1;
        }
    }
    assert!(emptied > 0, "the first boot kept no unit to empty");

    let second = super::bs::booted(dir.path());
    assert_eq!(ran(&second), Some(balaur_script::Value::Int(1)));
    drop(second);
    let _ = std::fs::remove_dir_all(&data);
}
