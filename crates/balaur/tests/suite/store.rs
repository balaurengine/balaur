//! `engine::store` and `engine::stored`: state no node owns, which one
//! script leaves and another reads.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use super::bs::{booted, field, one_script, ticked};

const KEEPER_BS: &str = "pub class Keeper { node: Node }
impl Keeper {
    fn init() {
        let round: any = { \"score\": 7, \"names\": [\"ana\"] };
        engine::store(\"round\", round);
    }
}
";

const READER_BS: &str = "pub class Reader { node: Node, score: int, first: str, absent: bool }
impl Reader {
    fn update(dt: float) {
        let round = engine::stored(\"round\");
        self.score = round[\"score\"].as_int();
        self.first = round[\"names\"][0].as_str();
        self.absent = engine::stored(\"nothing\") == nil;
    }
}
";

#[test]
fn a_value_one_script_stores_another_reads_back() {
    use balaur_script::Value::{Bool, Int, Str};
    let dir = one_script("Keeper", KEEPER_BS);
    let mut scene = std::fs::read_to_string(dir.path().join("main.toml")).unwrap();
    scene.push_str(
        "\n[[nodes]]\nid = \"reader\"\nname = \"Reader\"\nparent = \"keeper\"\nscript = { source = \"scripts/reader.bs\" }\n",
    );
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/reader.bs"), READER_BS).unwrap();
    let mut app = booted(dir.path());
    ticked(&mut app, 2);
    assert_eq!(field(&app, "Keeper/Reader", "score"), Some(Int(7)));
    assert_eq!(
        field(&app, "Keeper/Reader", "first"),
        Some(Str("ana".into()))
    );
    assert_eq!(field(&app, "Keeper/Reader", "absent"), Some(Bool(true)));
}
