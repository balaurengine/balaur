//! What a script hears when its file is rebuilt under a running scene.

#[test]
fn a_reload_tells_the_instances_of_that_file_and_keeps_their_state() {
    let dir = tempfile::tempdir().unwrap();
    let script = |class: &str| {
        format!(
            "pub class {class} {{ node: Node, pub reloads: int, pub kept: int }}\nimpl {class} {{\n    fn init() {{ self.kept = 7; }}\n    fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n}}\n"
        )
    };
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\nscript = { source = \"scripts/a.bs\" }\n\n[[nodes]]\nid = \"b\"\nname = \"B\"\nparent = \"a\"\nscript = { source = \"scripts/b.bs\" }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/a.bs"), script("A")).unwrap();
    std::fs::write(dir.path().join("scripts/b.bs"), script("B")).unwrap();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let field = |id: &str, name: &str| {
        let world = app.engine.world();
        let node = balaur_core::ids::find(&world, app.engine.root(), id).unwrap();
        host.script_field(balaur::node_id_of(node), name)
    };
    let int = balaur_script::Value::Int;

    host.reload(&["scripts/a.bs"]).unwrap();
    assert_eq!(
        field("a", "reloads"),
        Some(int(1)),
        "the file reloaded hears"
    );
    assert_eq!(field("a", "kept"), Some(int(7)), "with its state kept");
    assert_eq!(field("b", "reloads"), Some(int(0)), "another file does not");
    assert_eq!(
        field("b", "kept"),
        Some(int(7)),
        "though it was carried too"
    );
}

fn fixture_project(dir: &std::path::Path) {
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.toml"),
        "[[nodes]]\nid = \"a\"\nname = \"A\"\n",
    )
    .unwrap();
}

/// The lists `files_changed` carried in the pump just run.
fn said(app: &balaur::App) -> Vec<Vec<balaur_script::Value>> {
    balaur_core::events::delivered(&app.engine, balaur_core::events::FILES_CHANGED)
        .into_iter()
        .filter_map(|paths| match paths {
            balaur_script::Value::List(paths) => Some(paths),
            _ => None,
        })
        .collect()
}

fn watched_app(project: &std::path::Path) -> balaur::App {
    fixture_project(project);
    let mut app =
        balaur::standard_app(balaur::AppConfig::dev(project.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app
}

const SETTLED_TICKS: usize = 40;

/// Ticks past a burst's quiet time, for the frame after the last write.
fn settle(app: &mut balaur::App, mut each: impl FnMut(&balaur::App)) {
    for _ in 0..SETTLED_TICKS {
        app.tick(1.0 / 60.0);
        each(app);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_watched_directory_says_what_changed_and_reloads_its_scripts() {
    let project = tempfile::tempdir().unwrap();
    let mut app = watched_app(project.path());
    let game = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(game.path().join("scripts")).unwrap();
    let file = game.path().join("scripts/visitor.bs");
    let source = |version: u32| {
        format!(
            "pub class Visitor {{ node: Node, pub reloads: int }}\nimpl Visitor {{\n    fn on_hot_reload() {{ self.reloads = self.reloads + 1; }}\n    pub fn version() -> int {{ {version} }}\n}}\n"
        )
    };
    std::fs::write(&file, source(0)).unwrap();
    balaur::file_api::add_root(&app.engine, game.path());
    let host = app.engine.script_host().unwrap();
    let key = game.path().to_string_lossy().into_owned();
    let real = game.path().canonicalize().unwrap();
    assert!(host.watch(&real, &key, &[]), "this run follows files");
    let script = file.to_string_lossy().into_owned();
    let world = app.engine.world();
    let node = balaur_core::ids::find(&world, app.engine.root(), "a").unwrap();
    drop(world);
    let node = balaur::node_id_of(node);
    host.attach_with_props(node, &script, &[]).unwrap();

    let told = balaur_script::Value::Str(format!("{key}/scripts/visitor.bs"));
    let mut heard = false;
    let mut reloaded = false;
    // A watch begins a moment after it is asked for, so the file is written
    // again until something hears it.
    for attempt in 1..=80 {
        std::fs::write(&file, source(attempt)).unwrap();
        settle(&mut app, |app| {
            heard |= said(app).iter().any(|paths| paths.contains(&told));
            reloaded |= !matches!(
                host.script_field(node, "reloads"),
                Some(balaur_script::Value::Int(0)) | None
            );
        });
        if heard && reloaded {
            return;
        }
    }
    panic!("heard the change: {heard}, reloaded the script: {reloaded}");
}

#[test]
fn ten_thousand_files_changed_at_once_arrive_in_a_handful_of_lists() {
    const FILES: usize = 10_000;
    let project = tempfile::tempdir().unwrap();
    let mut app = watched_app(project.path());
    let game = tempfile::tempdir().unwrap();
    balaur::file_api::add_root(&app.engine, game.path());
    let host = app.engine.script_host().unwrap();
    let key = game.path().to_string_lossy().into_owned();
    let real = game.path().canonicalize().unwrap();
    assert!(host.watch(&real, &key, &["bs".to_string()]));

    let probe = game.path().join("probe.bs");
    let mut live = false;
    for attempt in 0..80 {
        std::fs::write(&probe, format!("// {attempt}")).unwrap();
        settle(&mut app, |app| live |= !said(app).is_empty());
        if live {
            break;
        }
    }
    assert!(live, "the watch never began");

    for at in 0..FILES {
        std::fs::write(game.path().join(format!("f{at}.bs")), "// a file").unwrap();
    }
    std::fs::write(game.path().join("skipped.png"), "not a script").unwrap();

    let mut lists = 0;
    let mut named: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut lost_count = false;
    for _ in 0..60 {
        settle(&mut app, |app| {
            for paths in said(app) {
                lists += 1;
                lost_count |= paths.is_empty();
                named.extend(paths.into_iter().map(|path| match path {
                    balaur_script::Value::Str(path) => path,
                    other => panic!("not a path: {other:?}"),
                }));
            }
        });
        if lost_count || named.len() >= FILES {
            break;
        }
    }
    assert!(
        lost_count || named.len() >= FILES,
        "{} of {FILES} heard",
        named.len()
    );
    assert!(lists <= FILES / 100, "{lists} lists for {FILES} files");
    assert!(!named.contains(&format!("{key}/skipped.png")));
}

#[test]
fn a_run_with_watching_off_follows_nothing() {
    let project = tempfile::tempdir().unwrap();
    fixture_project(project.path());
    let mut config = balaur::AppConfig::dev(project.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = balaur::standard_app(config).unwrap();
    app.load_project().unwrap();
    let host = app.engine.script_host().unwrap();
    let real = project.path().canonicalize().unwrap();
    assert!(!host.watch(&real, "elsewhere", &[]));
}
