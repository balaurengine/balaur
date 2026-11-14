//! The names `balaur check` holds a project to: a handler, a listener or a
//! row naming an event nothing announces, and an `emit` its script does not
//! declare.
//!
//! What the engine announces is read from where each plugin declares it
//! (`balaur_core::events::names`), so an event renamed there is renamed here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use balaur_core::events;
use balaur_script_bs::BsHost;

use super::Finding;

const TOML: &str = "toml";

/// What the project itself announces and calls by name, gathered from every
/// script and every data file under it, its dependencies included.
struct Project {
    engine: events::Names,
    /// Every module a script calls into: each may name an event after itself.
    modules: BTreeSet<String>,
    /// Every `event` a script of the project declares.
    declared: BTreeSet<String>,
    /// Every name a scene's `emit` row sends.
    row_sent: BTreeSet<String>,
    /// Every function and method a script calls or hands on, by name.
    called: BTreeSet<String>,
    /// Every string a script spells whole.
    strings: BTreeSet<String>,
    /// Every string a script builds around values, as the text between them.
    patterns: Vec<Vec<String>>,
    /// Each `.toml` under the project, as text: a scene's `call` row, a
    /// clip's method key and a pool's spec name a method there.
    data: Vec<String>,
}

impl Project {
    fn announces(&self, event: &str) -> bool {
        self.engine.announces(event)
            || self.declared.contains(event)
            || self.row_sent.contains(event)
            || event
                .strip_suffix(events::MODULE_EVENT_SUFFIX)
                .is_some_and(|module| self.modules.contains(module))
    }

    /// Whether anything names `method` as it is spelled: a call, a string in
    /// a script or a data file, or a string built to open as a handler does.
    fn calls_by_name(&self, method: &str) -> bool {
        if self.called.contains(method) || self.strings.contains(method) {
            return true;
        }
        let double = format!("\"{method}\"");
        let single = format!("'{method}'");
        self.patterns.iter().any(|pieces| fits(pieces, method))
            || self
                .data
                .iter()
                .any(|text| text.contains(&double) || text.contains(&single))
    }
}

/// Whether `text` could be the string `pieces` are the fixed text of, a value
/// between each two. Only a string that opens as a handler's name does is
/// read as one: `"on_{kind}_changed"`.
fn fits(pieces: &[String], text: &str) -> bool {
    let [first, between @ .., last] = pieces else {
        return false;
    };
    if !first.starts_with(events::HANDLER_PREFIX) || !text.starts_with(first.as_str()) {
        return false;
    }
    let mut rest = &text[first.len()..];
    for piece in between {
        match rest.find(piece.as_str()) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    rest.ends_with(last.as_str())
}

/// Every file under `dir` whose extension is one of `extensions`, by its
/// path from `root` and its text. A folder holding a manifest is another
/// project's, and is left alone.
fn files_under(
    fs: &dyn balaur_core::files::FileBackend,
    dir: &Path,
    prefix: &str,
    extensions: &[&str],
    out: &mut BTreeMap<String, String>,
) {
    let mut dirs = vec![(dir.to_path_buf(), prefix.to_string())];
    while let Some((dir, prefix)) = dirs.pop() {
        for (name, is_dir) in fs.list(&dir) {
            let path = dir.join(&name);
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if is_dir {
                if !name.starts_with('.')
                    && fs.read(&path.join(balaur_core::project::MANIFEST)).is_err()
                {
                    dirs.push((path, rel));
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
                && let Some(text) = fs.read(&path).ok().and_then(|b| String::from_utf8(b).ok())
            {
                out.insert(rel, text);
            }
        }
    }
}

/// The project's own files and each installed dependency's, by extension.
fn project_files(
    engine: &balaur_core::Engine,
    project_root: &Path,
    extensions: &[&str],
) -> BTreeMap<String, String> {
    let fs = balaur_core::files::backend(engine);
    let mut out = BTreeMap::new();
    files_under(&*fs, project_root, "", extensions, &mut out);
    for dependency in balaur_core::dependencies::installed(engine) {
        let prefix = balaur_core::dependencies::folder(&dependency.alias);
        files_under(&*fs, &dependency.root, &prefix, extensions, &mut out);
    }
    out
}

fn warning(file: &str, pos: balaur_script_bs::Pos, message: String) -> Finding {
    Finding::warning(file, pos.line as usize, pos.col as usize, message)
}

/// What a script hears that nothing announces, and sends without declaring.
fn script_findings(
    project: &Project,
    rel: &str,
    names: &balaur_script_bs::Names,
    found: &mut Vec<Finding>,
) {
    for (method, pos) in &names.methods {
        let Some(event) = method.strip_prefix(events::HANDLER_PREFIX) else {
            continue;
        };
        if project.engine.calls(method) || project.announces(event) || project.calls_by_name(method)
        {
            continue;
        }
        let message = format!(
            "`{method}` hears nothing: no hook, component, node or script announces `{event}`, and nothing calls the method by name"
        );
        found.push(warning(rel, *pos, message));
    }
    // A file that uses a module of its own named `events` calls that one.
    let engines = !names.modules.iter().any(|module| module == events::MODULE);
    let own_emit = names
        .declared_methods
        .iter()
        .any(|method| method == events::SENDING);
    for call in &names.calls {
        let function = call
            .callee
            .strip_prefix(events::MODULE)
            .and_then(|rest| rest.strip_prefix("::"))
            .filter(|_| engines && !call.method);
        let spelled = |index: usize| call.args.iter().find(|(at, ..)| *at == index);
        let heard = function
            .and_then(|function| events::HEARING.iter().find(|(name, _)| *name == function))
            .and_then(|(_, index)| spelled(*index));
        if let Some((_, event, pos)) = heard
            && !project.announces(event)
        {
            let message = format!(
                "nothing announces `{event}`: no hook, component or node sends it, and no script declares `event {event};`"
            );
            found.push(warning(rel, *pos, message));
        }
        let sends = if call.method {
            call.callee == events::SENDING && !own_emit
        } else {
            function == Some(events::SENDING)
        };
        if let Some((_, event, pos)) = spelled(0).filter(|_| sends)
            && !names.events.iter().any(|declared| declared.name == *event)
        {
            let message =
                format!("`{event}` is sent but not declared: write `event {event};` in this file");
            found.push(warning(rel, *pos, message));
        }
    }
}

/// The warnings about names, over the scripts `checked` and every scene.
pub(crate) fn findings(
    engine: &balaur_core::Engine,
    host: &BsHost,
    project_root: &Path,
    checked: &BTreeSet<String>,
) -> Vec<Finding> {
    use balaur_script::ScriptCompiler as _;
    let scripts = project_files(engine, project_root, host.extensions());
    let data = project_files(engine, project_root, &[TOML]);
    let mut project = Project {
        engine: events::names(engine),
        modules: host.module_names(),
        declared: BTreeSet::new(),
        row_sent: BTreeSet::new(),
        called: BTreeSet::new(),
        strings: BTreeSet::new(),
        patterns: Vec::new(),
        data: Vec::new(),
    };
    let mut read: BTreeMap<String, balaur_script_bs::Names> = BTreeMap::new();
    for (rel, source) in &scripts {
        let Some(names) = host.names(rel, source) else {
            continue;
        };
        project
            .declared
            .extend(names.events.iter().map(|event| event.name.clone()));
        project.called.extend(names.called.iter().cloned());
        project.strings.extend(names.strings.iter().cloned());
        project.patterns.extend(names.patterns.iter().cloned());
        read.insert(rel.clone(), names);
    }
    let mut rows = Vec::new();
    for scene in balaur_core::attachments::scene_files(project_root) {
        let Some(text) = data.get(&scene) else {
            continue;
        };
        let written = balaur_core::project::row_names(text);
        project.row_sent.extend(written.sent);
        rows.extend(
            written
                .heard
                .into_iter()
                .map(|heard| (scene.clone(), heard)),
        );
    }
    project.data = data.into_values().collect();
    let mut found = Vec::new();
    for rel in checked {
        if let Some(names) = read.get(rel) {
            script_findings(&project, rel, names, &mut found);
        }
    }
    for (scene, (event, line, column)) in rows {
        if !project.announces(&event) {
            let message = format!(
                "nothing announces `{event}`, which this row answers: no component or node sends it, and no script declares `event {event};`"
            );
            found.push(Finding::warning(&scene, line, column, message));
        }
    }
    found.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
    found
}
