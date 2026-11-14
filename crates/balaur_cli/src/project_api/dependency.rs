//! `dependency.*`, the half that writes: installing what a game declares,
//! declaring a `path` dependency, and moving files between a game and what it
//! imports. The engine's half reads, `list` and `of`, which a game's runtime
//! has; this half is the editor's.

// A raw binding answers a `Result`, whether or not it can fail.
#![allow(clippy::unnecessary_wraps)]

use anyhow::{Result, anyhow, bail};
use balaur::Engine;
use balaur::dependencies::{folder, install, installed, valid_alias};
use balaur::engine_api::text;
use balaur_script::{Module, Value};

use crate::vocabulary::keys;

const ADD_PATH: &str = "add_path";
const DEPENDENCIES: &str = "dependencies";
const DEPENDENCY: &str = "dependency";
const INSTALL: &str = "install";
const MAKE_LOCAL: &str = "make_local";
const MOVE_INTO: &str = "move_into";

pub(super) fn declare(reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
    let mut m = reg.script_module(DEPENDENCY)?.editor_only();
    install_dependency_verbs(&mut m);
    Ok(())
}

fn install_dependency_verbs(m: &mut Module<Engine>) {
    m.describe(&[
        (MAKE_LOCAL, &[], "(from: string, to: string) -> [string]", "Copy a dependency's file into the game and point the game's references at the copy; answers the game's files rewritten. The copy never updates. A document's own paths come out as the game names the dependency's files."),
        (MOVE_INTO, &[], "(paths: [string], alias: string) -> [string]", "Move game files into a `path` dependency, each at the same place under its root, and point the game's references at `deps/<alias>/…`; answers the files rewritten."),
        (ADD_PATH, &[], "(game: string, alias: string, path: string)", "Declare `alias = { path = \"<path>\" }` in a game's `[dependencies]`, comments kept, and install it. An empty game is the open project."),
        (INSTALL, &[], "(game: string)", "Read a game's `[dependencies]` again and install each: what a dependency fetched while the editor runs needs to become a root. An empty game is the open project."),
    ]);
    m.function_raw(MAKE_LOCAL, Box::new(make_local));
    m.function_raw(MOVE_INTO, Box::new(move_into));
    m.function_raw(ADD_PATH, Box::new(add_path));
    m.function_raw(INSTALL, Box::new(reinstall));
}

fn paths(rewritten: Vec<String>) -> Value {
    Value::List(rewritten.into_iter().map(Value::text).collect())
}

fn make_local(eng: &Engine, args: &[Value]) -> Result<Value> {
    balaur::asset_index::make_local(eng, text(args, 0)?, text(args, 1)?).map(paths)
}

fn move_into(eng: &Engine, args: &[Value]) -> Result<Value> {
    let Some(Value::List(items)) = args.first() else {
        bail!("argument 0 should be a list of paths");
    };
    let moved = items
        .iter()
        .map(|item| match item {
            Value::Str(path) => Ok(path.clone()),
            other => Err(anyhow!("a path should be a string, got {other:?}")),
        })
        .collect::<Result<Vec<_>>>()?;
    balaur::asset_index::move_into(eng, &moved, text(args, 1)?).map(paths)
}

/// The game a verb names, or the open project for an empty name.
fn game_at(eng: &Engine, game: &str) -> Result<std::path::PathBuf> {
    if !game.is_empty() {
        return Ok(std::path::PathBuf::from(game));
    }
    balaur::file_api::project_roots(eng)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no project is open"))
}

/// Read a game's `[dependencies]` again and install what it declares: a
/// dependency fetched while the editor runs.
fn reinstall(eng: &Engine, args: &[Value]) -> Result<Value> {
    install(eng, &game_at(eng, text(args, 0)?)?);
    Ok(Value::Nil)
}

/// Declare a `path` dependency in a game's `project.toml`, comments kept,
/// and install it.
fn add_path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (game, alias, path) = (text(args, 0)?, text(args, 1)?, text(args, 2)?);
    if !valid_alias(alias) {
        bail!("`{alias}` is not a dependency name: lowercase words joined by `_`");
    }
    let game = game_at(eng, game)?;
    let backend = balaur::files::backend(eng);
    let manifest = game.join(balaur::project::MANIFEST);
    let before = String::from_utf8(backend.read(&manifest)?)?;
    let mut document: toml_edit::DocumentMut = before.parse()?;
    let table = document
        .entry(DEPENDENCIES)
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or_else(|| anyhow!("[dependencies] is not a table"))?;
    if table.contains_key(alias) {
        bail!("the game already has a dependency named `{alias}`");
    }
    let mut entry = toml_edit::InlineTable::new();
    entry.insert(keys::PATH, path.into());
    table.insert(alias, toml_edit::value(entry));
    backend.write(&manifest, document.to_string().as_bytes())?;
    install(eng, &game);
    if !installed(eng).iter().any(|dep| dep.alias == alias) {
        bail!("{} did not install", folder(alias));
    }
    Ok(Value::Nil)
}

#[cfg(test)]
#[cfg(all(not(target_family = "wasm"), any(feature = "bs", feature = "bs-jit")))]
mod tests {
    use balaur::AppConfig;
    use balaur_script::Value::{Int, Str};

    const ASKER: &str = r#"pub class Asker {
    node: Node,
    before: int,
    after: int,
    alias: str,
    owner: str,
    copied: int,
    moved: int,
}

impl Asker {
    fn init() {
        self.before = dependency::list().len();
        dependency::add_path("", "kit", "../kit");
        let found = dependency::list();
        self.after = found.len();
        self.alias = found[0].alias;
        self.owner = dependency::of("deps/kit/art/hit.png").or("");
        dependency::install("");
        self.copied = dependency::make_local("deps/kit/art/hit.png", "art/hit.png").len();
        self.moved = dependency::move_into(["art/old.png"], "kit").len();
    }
}
"#;

    /// A game at `<tmp>/game` beside the project at `<tmp>/kit` it is about
    /// to depend on, with a document naming a file of each.
    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            (
                "game/project.toml",
                "# the game\n[application]\nname = \"game\"\nmain_scene = \"main.toml\"\n",
            ),
            (
                "game/main.toml",
                "[[nodes]]\nid = \"asker\"\nname = \"Asker\"\nscript = { source = \"scripts/asker.bs\" }\n",
            ),
            ("game/scripts/asker.bs", ASKER),
            ("game/art/old.png", "old"),
            (
                "game/notes/names.toml",
                "theirs = \"deps/kit/art/hit.png\"\nours = \"art/old.png\"\n",
            ),
            (
                "kit/project.toml",
                "[application]\nname = \"kit\"\nmain_scene = \"main.toml\"\n",
            ),
            ("kit/main.toml", ""),
            ("kit/art/hit.png", "hit"),
        ];
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    #[test]
    fn a_script_adds_installs_lists_and_moves_into_a_dependency() {
        let dir = workspace();
        let game = dir.path().join("game").canonicalize().unwrap();
        let mut app =
            balaur::standard_app(AppConfig::dev(game.to_string_lossy().as_ref())).unwrap();
        // The app runs `game` as the editor, whose scripts call these verbs.
        let modules = &mut crate::own_modules(&game, Some(game.clone()));
        balaur_plugin::load_all(&mut app, modules).unwrap();
        app.load_project().unwrap();
        app.tick(balaur::fixed_dt());
        let field = |name: &str| {
            let node = {
                let world = app.engine.world();
                balaur::scene::find_node(&world, app.engine.root(), "Asker").unwrap()
            };
            let host = app.engine.script_host().unwrap();
            host.script_field(balaur::node_id_of(node), name)
        };
        assert_eq!(field("before"), Some(Int(0)));
        assert_eq!(field("after"), Some(Int(1)), "add_path installs it");
        assert_eq!(field("alias"), Some(Str("kit".into())));
        assert_eq!(field("owner"), Some(Str("kit".into())));
        assert_eq!(field("copied"), Some(Int(1)));
        assert_eq!(field("moved"), Some(Int(1)));
        let manifest = std::fs::read_to_string(game.join("project.toml")).unwrap();
        assert!(manifest.starts_with("# the game"), "{manifest}");
        assert!(
            manifest.contains("kit = { path = \"../kit\" }"),
            "{manifest}"
        );
        assert_eq!(std::fs::read(game.join("art/hit.png")).unwrap(), b"hit");
        assert!(!game.join("art/old.png").exists());
        assert_eq!(
            std::fs::read(dir.path().join("kit/art/old.png")).unwrap(),
            b"old"
        );
        let names = std::fs::read_to_string(game.join("notes/names.toml")).unwrap();
        assert!(names.contains("theirs = \"art/hit.png\""), "{names}");
        assert!(names.contains("ours = \"deps/kit/art/old.png\""), "{names}");
    }
}
