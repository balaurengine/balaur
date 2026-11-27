//! An answer woken before anything waits on its token is kept for the wait.

use super::bs::{booted, field, one_script, ticked};

const PARSERS_BS: &str = r#"pub class Parsers { node: Node, sum: int, waited: int }

impl Parsers {
    pub async fn init() {
        let ids: [int] = [];
        for i in 0..20 {
            ids.push(json::parse_async("{{\"n\": {i}}}"));
        }
        await task::frames(60);
        self.waited = 1;
        for id in ids {
            let value = await task::wait(id);
            self.sum += value["n"].as_int();
        }
    }
}
"#;

#[test]
fn twenty_tasks_answered_before_anything_waits_are_each_kept_for_their_wait() {
    use balaur_script::Value::Int;
    let dir = one_script("Parsers", PARSERS_BS);
    let mut app = booted(dir.path());
    for _ in 0..500 {
        if field(&app, "Parsers", "sum") == Some(Int(190)) {
            break;
        }
        ticked(&mut app, 1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        field(&app, "Parsers", "waited"),
        Some(Int(1)),
        "the script ran"
    );
    assert_eq!(field(&app, "Parsers", "sum"), Some(Int(190)));
}

const KEEPER_BS: &str = "pub class Keeper { node: Node, id: int }

impl Keeper {
    pub fn init() {
        self.id = json::parse_async(\"1\");
    }

    pub fn asked() -> int {
        self.id
    }
}
";

const TAKER_BS: &str = "pub class Taker { node: Node, kept: int, waiting: int, dropped: int }

impl Taker {
    pub async fn init() {
        await task::frames(120);
        let kept = self.node.get_node(\"Keeper\").call(\"asked\").as_int();
        let leaver = self.node.get_node(\"Leaver\");
        let dropped = leaver.call(\"asked\").as_int();
        leaver.queue_free();
        await task::frames(2);
        self.node.call(\"await_dropped\", dropped);
        let value = await task::wait(kept);
        self.kept = value.as_int();
    }

    pub async fn await_dropped(id: int) {
        self.waiting = 1;
        await task::wait(id);
        self.dropped = 1;
    }
}
";

#[test]
fn an_answer_nothing_waited_on_is_let_go_of_with_the_node_that_asked() {
    use balaur_script::Value::Int;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let scene = "[[nodes]]\nid = \"taker\"\nname = \"Taker\"\nscript = { source = \"scripts/taker.bs\" }\n\n\
                 [[nodes]]\nid = \"keeper\"\nname = \"Keeper\"\nparent = \"taker\"\nscript = { source = \"scripts/keeper.bs\" }\n\n\
                 [[nodes]]\nid = \"leaver\"\nname = \"Leaver\"\nparent = \"taker\"\nscript = { source = \"scripts/leaver.bs\" }\n";
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/keeper.bs"), KEEPER_BS).unwrap();
    std::fs::write(
        dir.path().join("scripts/leaver.bs"),
        KEEPER_BS.replace("Keeper", "Leaver"),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/taker.bs"), TAKER_BS).unwrap();
    let mut app = booted(dir.path());
    for _ in 0..500 {
        if field(&app, "Taker", "kept") == Some(Int(1)) {
            break;
        }
        ticked(&mut app, 1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        field(&app, "Taker", "kept"),
        Some(Int(1)),
        "kept for the node that asked, and taken by any wait"
    );
    ticked(&mut app, 30);
    assert_eq!(
        field(&app, "Taker", "waiting"),
        Some(Int(1)),
        "the wait ran"
    );
    assert_eq!(field(&app, "Taker", "dropped"), Some(Int(0)));
}
