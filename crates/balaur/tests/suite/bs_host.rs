//! How the `.bs` host runs a scene's scripts: a call back into a script that
//! is busy, a script taken away mid-frame, and the profiler's rebuilds.

#![cfg(any(feature = "bs", feature = "bs-jit"))]

use super::bs::{booted, field, project, spun, ticked};

const PAIR: &str = "pub class Pair { node: Node, hits: int, answer: int, seen: int }
impl Pair {
    fn update(dt: float) {
        if self.node.name() == \"Outer\" {
            let inner = self.node.get_node(\"Inner\");
            self.answer = inner.call(\"hit\", 5).as_int();
            self.seen = inner.script_field(\"hits\").as_int();
        }
    }
    pub fn hit(amount: any) -> int {
        self.hits += amount.as_int();
        self.hits
    }
}
";

#[test]
fn a_script_calls_a_node_running_its_own_file() {
    use balaur_script::Value::Int;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"outer\"\nname = \"Outer\"\nscript = { source = \"scripts/pair.bs\" }\n\n\
         [[nodes]]\nid = \"inner\"\nname = \"Inner\"\nparent = \"outer\"\nscript = { source = \"scripts/pair.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/pair.bs"), PAIR).unwrap();
    let mut app = booted(dir.path());
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Outer", "answer"), Some(Int(5)));
    assert_eq!(field(&app, "Outer", "seen"), Some(Int(5)));
    assert_eq!(field(&app, "Outer/Inner", "hits"), Some(Int(5)));
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Outer", "answer"), Some(Int(10)));
    assert_eq!(field(&app, "Outer/Inner", "hits"), Some(Int(10)));
}

// `Host` calls `Guest`, which calls `Host` back while `Host`'s call runs:
// the plugin answering the Editor and calling the Editor back.
const HOST: &str = "pub class Count { n: int }

pub class Host {
    node: Node,
    count: Count,
    seen: int,
}

impl Host {
    fn init() {
        self.count = Count { n: 0 };
    }
    fn update(dt: float) {
        self.node.get_node(\"Guest\").call(\"visit\");
        self.seen = self.count.n;
    }
    pub fn note(by: int) {
        self.count.n += by;
    }
}
";

const GUEST: &str = "pub class Guest {
    node: Node,
}

impl Guest {
    pub fn visit() {
        self.node.parent().call(\"note\", 3);
    }
}
";

#[test]
fn a_call_back_into_a_busy_script_shares_the_class_it_keeps() {
    use balaur_script::Value::Int;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"host\"\nname = \"Host\"\nscript = { source = \"scripts/host.bs\" }\n\n\
         [[nodes]]\nid = \"guest\"\nname = \"Guest\"\nparent = \"host\"\nscript = { source = \"scripts/guest.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/host.bs"), HOST).unwrap();
    std::fs::write(dir.path().join("scripts/guest.bs"), GUEST).unwrap();
    let mut app = booted(dir.path());
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Host", "seen"), Some(Int(3)));
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Host", "seen"), Some(Int(6)));
}

// Whichever of `First` and `Second` runs first takes the other's script away in the
// same frame, while the pass that runs them both still holds it: what a
// replay's scene reset does to a game while the editor's call runs.
const RIVAL: &str = "pub class Rival {
    node: Node,
    said: str,
}

impl Rival {
    fn update(dt: float) {
        if self.said == \"\" {
            let other = if self.node.name() == \"First\" { \"Second\" } else { \"First\" };
            self.node.parent().get_node(other).detach_script();
        }
        self.said = \"here\";
    }
}
";

#[test]
fn a_script_detached_in_a_frame_is_not_called_later_in_it() {
    balaur_core::logbuf::capture_for_test();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"root\"\nname = \"Root\"\n\n\
         [[nodes]]\nid = \"first\"\nname = \"First\"\nparent = \"root\"\nscript = { source = \"scripts/rival.bs\" }\n\n\
         [[nodes]]\nid = \"second\"\nname = \"Second\"\nparent = \"root\"\nscript = { source = \"scripts/rival.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/rival.bs"), RIVAL).unwrap();
    let mut app = booted(dir.path());
    // A frame at a time: the buffer keeps 500 lines, and the engine's trace
    // lines would push an error out of it over two.
    for _ in 0..2 {
        ticked(&mut app, 1);
        let errors: Vec<_> = balaur_core::logbuf::recent(500)
            .into_iter()
            .filter(|e| e.level == "error" && e.message.contains("rival.bs"))
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }
}

#[test]
fn a_profiler_rebuilds_the_scripts_with_counters_and_without() {
    use balaur_script::ScriptHost as _;
    let dir = project("spinner.bs");
    let mut app = booted(dir.path());
    ticked(&mut app, 2);
    let host = balaur::script_bs::BsHost::of(&app.engine).unwrap();
    assert!(host.script_costs().is_empty(), "nothing counts until asked");
    host.set_profiling(true);
    ticked(&mut app, 3);
    let spinner = host
        .script_costs()
        .into_iter()
        .find(|(path, _, _)| path == "scripts/spinner.bs")
        .expect("the spinner is counted once rebuilt");
    assert!(spinner.1 >= 2 && spinner.2 > 0, "{spinner:?}");
    let update = host
        .function_costs()
        .into_iter()
        .find(|(name, _, _)| name == "Spinner.update")
        .expect("its update is a row");
    assert!(update.1 >= 2 && update.2 > 0, "{update:?}");
    // On again starts the count again; off rebuilds without counters.
    host.set_profiling(true);
    assert!(host.script_costs().is_empty());
    assert!(host.function_costs().is_empty());
    host.set_profiling(false);
    ticked(&mut app, 2);
    assert!(host.script_costs().is_empty());
    assert!(host.function_costs().is_empty());
    // The rebuilds carried the spinner across: it still turns.
    let before = spun(&app);
    ticked(&mut app, 2);
    assert_ne!(spun(&app), before);
}

const EDITOR: &str = "pub class Editor { node: Node, edits: int }
impl Editor {
    pub fn edit() -> int {
        self.edits += 1;
        self.edits
    }
}
";

const PANEL: &str = "use editor;
pub class Panel { node: Node, editor: editor::Editor?, seen: int }
impl Panel {
    fn init() {
        self.editor = self.node.parent().script;
    }
    fn update(dt: float) {
        if self.editor.is_some() {
            self.seen = self.editor.unwrap().edit();
        }
    }
}
";

/// The scene a panel's node under the editor's makes: the editor's script
/// in `scripts/editor.bs`, the panel's beside it.
fn editor_and_panel(editor: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"editor\"\nname = \"Editor\"\nscript = { source = \"scripts/editor.bs\" }\n\n\
         [[nodes]]\nid = \"panel\"\nname = \"Panel\"\nparent = \"editor\"\nscript = { source = \"scripts/panel.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/editor.bs"), editor).unwrap();
    std::fs::write(dir.path().join("scripts/panel.bs"), PANEL).unwrap();
    dir
}

// The project's scripts are units of one instance: a panel holds the
// editor's own object, read from its node as the class it is, and calls it
// with nothing copied. A reload of the editor's file keeps the panel holding
// the one editor rather than a copy of it.
#[test]
fn a_script_holds_another_nodes_script_as_its_class() {
    use balaur_script::Value::Int;
    let dir = editor_and_panel(EDITOR);
    let mut app = booted(dir.path());
    ticked(&mut app, 3);
    assert_eq!(field(&app, "Editor/Panel", "seen"), Some(Int(3)));
    assert_eq!(field(&app, "Editor", "edits"), Some(Int(3)));

    std::fs::write(
        dir.path().join("scripts/editor.bs"),
        EDITOR.replace("self.edits += 1;", "self.edits += 10;"),
    )
    .unwrap();
    let host = app.engine.script_host().unwrap();
    host.reload("scripts/editor.bs").unwrap();
    ticked(&mut app, 1);
    assert_eq!(field(&app, "Editor", "edits"), Some(Int(13)));
    assert_eq!(field(&app, "Editor/Panel", "seen"), Some(Int(13)));
}
