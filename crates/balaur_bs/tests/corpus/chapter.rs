// The language chapter, `docs/language/`, read fence by fence: `balaur_bs`'s
// suite answers for a fence that needs no engine API, `balaur`'s for the rest.
// `docs/QUALITY.md` says what a fence may claim in its comments.

use std::path::{Path, PathBuf};

pub(crate) struct Page {
    /// Its path in the chapter.
    pub(crate) name: String,
    /// Each fence: the line its code starts on, and the code.
    pub(crate) fences: Vec<(usize, String)>,
}

/// Every page of the chapter, in path order.
pub(crate) fn pages() -> Vec<Page> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/language");
    let mut paths = vec![];
    markdown(&root, &mut paths);
    paths.sort();
    paths
        .iter()
        .map(|path| Page {
            name: path
                .strip_prefix(&root)
                .unwrap()
                .display()
                .to_string()
                .replace('\\', "/"),
            fences: fences(&std::fs::read_to_string(path).unwrap()),
        })
        .collect()
}

fn markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn fences(text: &str) -> Vec<(usize, String)> {
    let (mut out, mut open, mut at) = (vec![], None::<String>, 0);
    for (index, line) in text.lines().enumerate() {
        let bare = line.trim_start();
        match &mut open {
            None if bare == "```bs" => {
                open = Some(String::new());
                at = index + 2;
            }
            Some(_) if bare == "```" => out.extend(open.take().map(|code| (at, code))),
            Some(code) => {
                code.push_str(line);
                code.push('\n');
            }
            None => {}
        }
    }
    out
}

/// The module a fence's first line names it as: `util.bs`, `addons/kit/core.bs`.
pub(crate) fn module_name(code: &str) -> Option<&str> {
    code.lines().next()?.strip_prefix("// ").filter(|name| {
        Path::new(name).extension().is_some_and(|e| e == "bs") && !name.contains(' ')
    })
}

/// A fence that needs the engine's API to compile.
pub(crate) fn needs_the_engine(code: &str) -> bool {
    const ENGINE: [&str; 21] = [
        "self.node",
        "node.",
        "node:",
        "log::",
        "input::",
        "task::",
        "events::",
        "math::",
        "types::",
        "gamend::",
        "http::",
        "scene::",
        "engine::",
        "json::",
        "fs::",
        "regex::",
        "time::",
        "random::",
        "audio::",
        "ui::",
        "physics",
    ];
    !code.contains("// error:") && ENGINE.iter().any(|word| code.contains(word))
}

/// A fence with no item at column 0.
pub(crate) fn is_fragment(code: &str) -> bool {
    const ITEMS: [&str; 10] = [
        "fn ", "pub ", "struct ", "class ", "enum ", "impl ", "const ", "use ", "async ", "extern ",
    ];
    !code
        .lines()
        .any(|line| ITEMS.iter().any(|item| line.starts_with(item)))
}
