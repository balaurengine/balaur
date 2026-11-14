//! What building a scene refuses, kept for a checker where a run logs it: one
//! loader answers both, so a check and a load cannot disagree.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;
use crate::vocabulary::keys;

/// The node a checker builds a scene under, and frees with it.
const SCRATCH: &str = "scene_check";

/// One thing the build of a scene refused: what a run logs as it loads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The scene file it is written in: the one built, or a prefab under it.
    pub file: String,
    /// Where in that file, from 1; 0 for the file as a whole.
    pub line: usize,
    pub column: usize,
    /// Names the node, and the component and the property where it has one.
    pub message: String,
    /// A run only warns of it: this host lacks a module that may own the key.
    pub warning: bool,
}

/// A refusal as the build met it, until its file is read for the line.
struct Refused {
    prefab: Option<String>,
    index: usize,
    path: Vec<String>,
    message: String,
    warning: bool,
}

/// The refusals of the scene [`problems`] is building; absent in a run.
#[derive(Default)]
struct Kept(Vec<Refused>);

/// How bad a refusal is: what a run logs it as.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Level {
    Error,
    Warning,
}

/// Whether [`problems`] is building this scene: nothing is run from it, so
/// advice about saving it has no reader.
pub(super) fn checking(eng: &Engine) -> bool {
    eng.try_resource::<Kept>().is_some()
}

/// Keep a refusal for the checker building this scene. `false` in a run,
/// which logs it instead. `path` is the keys down from the node's own table,
/// and `index` is which `[[nodes]]` of its file the node is.
pub(super) fn keep(
    eng: &Engine,
    build: &Build,
    index: usize,
    path: &[&str],
    level: Level,
    message: &str,
) -> bool {
    let Some(kept) = eng.try_resource::<Kept>() else {
        return false;
    };
    let prefab = build
        .open
        .last()
        .map(|prefab| path_of(eng, prefab).unwrap_or_else(|_| prefab.clone()));
    kept.borrow_mut().0.push(Refused {
        prefab,
        index,
        path: path.iter().map(ToString::to_string).collect(),
        message: message.to_string(),
        warning: level == Level::Warning,
    });
    true
}

/// The property of `written` a component's refusal is about: the one its
/// schema lacks, or the one the component's read names above the value's error.
pub(super) fn property_of(
    eng: &Engine,
    component: &str,
    written: &toml::Value,
    why: &anyhow::Error,
) -> Option<String> {
    let table = written.as_table()?;
    crate::components::unknown_key(eng, component, Some(written)).or_else(|| {
        why.chain()
            .map(ToString::to_string)
            .find(|link| table.contains_key(link))
    })
}

/// Everything building the scene at `path` refuses, with no script run: the
/// tree is built under a node of its own with its scripts held, then freed.
///
/// A prefab's own refusals name the prefab's file, so a caller walking every
/// scene meets them once per instance and once more for the file itself.
#[must_use]
pub fn problems(eng: &Engine, path: &str) -> Vec<Problem> {
    eng.insert_resource(Kept::default());
    let base = eng.spawn_node(SCRATCH, eng.root());
    let built = instantiate_scene_file(eng, path, base, false);
    scene::free_node(eng, base);
    let kept = eng
        .try_resource::<Kept>()
        .map(|kept| std::mem::take(&mut kept.borrow_mut().0))
        .unwrap_or_default();
    eng.remove_resource::<Kept>();
    let mut found: Vec<Problem> = kept
        .into_iter()
        .map(|refused| {
            let file = refused.prefab.unwrap_or_else(|| path.to_string());
            let (line, column) = scene_doc(eng, &file).map_or((0, 0), |(source, _)| {
                place_of(&source, refused.index, &refused.path)
            });
            Problem {
                file,
                line,
                column,
                message: refused.message,
                warning: refused.warning,
            }
        })
        .collect();
    if let Err(why) = built {
        found.push(Problem {
            file: path.to_string(),
            line: 0,
            column: 0,
            message: format!("{why:#}"),
            warning: false,
        });
    }
    found
}

/// The line and column, from 1, of the deepest key of `path` that node
/// `index` of the scene text holds: a property the file does not write
/// answers with its component, and a component with its node. `(0, 0)` when
/// the text has no such node.
fn place_of(source: &str, index: usize, path: &[String]) -> (usize, usize) {
    let Ok(doc) = toml_edit::Document::parse(source) else {
        return (0, 0);
    };
    let Some(node) = doc
        .as_table()
        .get(keys::NODES)
        .and_then(toml_edit::Item::as_array_of_tables)
        .and_then(|nodes| nodes.get(index))
    else {
        return (0, 0);
    };
    let mut span = node.span();
    let mut table: Option<&dyn toml_edit::TableLike> = Some(node);
    for key in path {
        let Some((found, item)) = table.and_then(|table| table.get_key_value(key)) else {
            break;
        };
        span = found.span().or(span);
        table = item.as_table_like();
    }
    let Some(span) = span else {
        return (0, 0);
    };
    let before = &source[..span.start.min(source.len())];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

/// The key a binding row names its event under, and the one for its action.
const EVENT: &str = "event";
const ACTION: &str = "action";

/// What the binding rows of one scene file name, read from its text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowNames {
    /// Each `emitted:<name>` a row answers: the name, then the line and
    /// column, from 1, that spell it.
    pub heard: Vec<(String, usize, usize)>,
    /// Each name a row's `emit` action sends from its node.
    pub sent: Vec<String>,
}

/// The events the rows of the scene text `source` hear and send, wherever a
/// row is written: under `[[nodes.bindings.rows]]` or inline. Empty for a
/// text that does not parse, which is the loader's error to report.
#[must_use]
pub fn row_names(source: &str) -> RowNames {
    let mut found = RowNames::default();
    if let Ok(doc) = toml_edit::Document::parse(source) {
        for (_, item) in doc.as_table() {
            rows_in_item(item, source, &mut found);
        }
    }
    found
}

fn rows_in_item(item: &toml_edit::Item, source: &str, found: &mut RowNames) {
    match item {
        toml_edit::Item::Table(table) => {
            row(table, source, found);
            for (_, child) in table {
                rows_in_item(child, source, found);
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables {
                row(table, source, found);
                for (_, child) in table {
                    rows_in_item(child, source, found);
                }
            }
        }
        toml_edit::Item::Value(value) => rows_in_value(value, source, found),
        toml_edit::Item::None => {}
    }
}

fn rows_in_value(value: &toml_edit::Value, source: &str, found: &mut RowNames) {
    match value {
        toml_edit::Value::InlineTable(table) => {
            row(table, source, found);
            for (_, child) in table {
                rows_in_value(child, source, found);
            }
        }
        toml_edit::Value::Array(items) => {
            for child in items {
                rows_in_value(child, source, found);
            }
        }
        _ => {}
    }
}

// One table, read as a row where it has a row's keys.
fn row(table: &dyn toml_edit::TableLike, source: &str, found: &mut RowNames) {
    let text = |key: &str| table.get(key).and_then(toml_edit::Item::as_str);
    if let Some(name) = text(EVENT).and_then(|event| event.strip_prefix(crate::hooks::EMITTED)) {
        let at = table
            .get(EVENT)
            .and_then(toml_edit::Item::span)
            .map_or(0, |span| span.start);
        let (line, column) = line_column(source, at);
        found.heard.push((name.to_string(), line, column));
    }
    if text(ACTION) == Some(crate::bindings::Action::Emit.word())
        && let Some(name) = text(keys::VALUE)
    {
        found.sent.push(name.to_string());
    }
}

/// The line and column, from 1, of byte `at` of `source`.
fn line_column(source: &str, at: usize) -> (usize, usize) {
    let before = &source[..at.min(source.len())];
    let line_start = before.rfind('\n').map_or(0, |found| found + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::place_of;

    const SCENE: &str = "[[nodes]]\nname = \"Root\"\n\n[[nodes]]\nname = \"Ball\"\nbody2d = { kind = \"wobbly\" }\n\n[nodes.collider2d]\nrestitution = 0.85\n";

    fn path(keys: &[&str]) -> Vec<String> {
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_property_is_placed_on_the_line_that_writes_it() {
        let at = place_of(SCENE, 1, &path(&["collider2d", "restitution"]));
        assert_eq!(at, (9, 1));
        let at = place_of(SCENE, 1, &path(&["body2d", "kind"]));
        assert_eq!(at, (6, 12));
    }

    #[test]
    fn a_property_the_file_does_not_write_is_placed_on_its_component() {
        let at = place_of(SCENE, 1, &path(&["collider2d", "shape"]));
        assert_eq!(at.0, 8);
    }

    #[test]
    fn a_node_the_file_does_not_have_has_no_place() {
        assert_eq!(place_of(SCENE, 7, &path(&["collider2d"])), (0, 0));
    }
}
