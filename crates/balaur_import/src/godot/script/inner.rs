//! A file's inner `class Name:` blocks, each a script of its own that sees
//! what the class around it sees, and what the class index reads of a file.

use std::collections::BTreeMap;

use super::{declarations, name_of, split_functions, top_level};

/// A file's inner `class Name:` blocks, each as the source of a script of its
/// own: `extends` its base, or `RefCounted`, then its body one level out.
pub(crate) fn inner_classes(source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some(rest) = line.strip_prefix("class ") else {
            continue;
        };
        let head = rest
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches(':');
        let name = name_of(head);
        let base = head
            .split_once(" extends ")
            .map_or("RefCounted", |(_, base)| base.trim());
        let mut body: Vec<&str> = Vec::new();
        while i < lines.len() && (lines[i].trim().is_empty() || lines[i].starts_with([' ', '\t'])) {
            body.push(lines[i]);
            i += 1;
        }
        let unit = body
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        let mut text = format!("extends {base}\n");
        for l in body {
            text.push_str(l.get(unit..).unwrap_or_default());
            text.push('\n');
        }
        out.push((name, text));
    }
    out
}

/// A file's functions with defaulted parameters, and how many each takes.
/// Every signal a file declares, with how many values it carries.
pub(crate) fn signal_arities(source: &str) -> BTreeMap<String, usize> {
    declarations(source).signal_arity
}

/// Every member variable a file declares.
pub(crate) fn member_names(source: &str) -> std::collections::BTreeSet<String> {
    declarations(source).members
}

/// Every function a file declares, under the Rune name it is emitted with.
pub(crate) fn function_names(source: &str) -> std::collections::BTreeSet<String> {
    split_functions(source)
        .into_iter()
        .map(|f| {
            if f.name == "_init" {
                "new".to_string()
            } else {
                f.name
            }
        })
        .collect()
}

pub(crate) fn defaulted(source: &str) -> BTreeMap<String, usize> {
    split_functions(source)
        .into_iter()
        .filter(|f| f.defaults.iter().any(Option::is_some))
        // `new` takes what `_init` takes.
        .map(|f| {
            (
                if f.name == "_init" {
                    "new".to_string()
                } else {
                    f.name
                },
                f.params.len(),
            )
        })
        .collect()
}

/// Where an inner class of `file` is written: beside it, named for both.
pub(crate) fn inner_file(file: &str, name: &str) -> String {
    format!("{}__{name}.gd", file.trim_end_matches(".gd"))
}

/// A file's inner classes as scripts that see what the class around them
/// sees: each sibling as a preloaded class, and the outer's constants and
/// enums after the body, so the inner's own declaration of a name wins.
pub(crate) fn inner_scripts(source: &str, file: &str) -> Vec<(String, String)> {
    use std::fmt::Write as _;
    let inners = inner_classes(source);
    let shared: Vec<String> = top_level(source)
        .into_iter()
        .filter(|line| line.starts_with("const ") || line.starts_with("enum "))
        .collect();
    inners
        .iter()
        .map(|(name, text)| {
            let own = declarations(text);
            // Its own name after `extends`, so `Msg.Part.new()` inside `Msg`
            // reaches the module; nothing on disk carries it otherwise.
            let mut out = text.replacen('\n', &format!("\nclass_name {name}\n"), 1);
            for (sibling, _) in &inners {
                if sibling != name {
                    let _ = writeln!(
                        out,
                        "const {sibling} = preload(\"res://{}\")",
                        inner_file(file, sibling)
                    );
                }
            }
            for line in &shared {
                let rest = line
                    .strip_prefix("const ")
                    .or_else(|| line.strip_prefix("enum "))
                    .unwrap_or(line);
                let declared = name_of(rest);
                if own.consts.contains(&declared) || own.lazy.contains(&declared) {
                    continue;
                }
                out.push_str(line);
                out.push('\n');
            }
            (name.clone(), out)
        })
        .collect()
}
