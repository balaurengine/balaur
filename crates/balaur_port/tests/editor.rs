// Every file of the editor ports, together, without a panic, and the report
// accounts for every line of code.

use balaur_port::{Api, Input, port_files};
use std::path::{Path, PathBuf};

fn rune_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rune_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rn") {
            out.push(p);
        }
    }
}

#[test]
fn the_whole_editor_ports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let editor = root.join("editor");
    let mut paths = vec![];
    rune_files(&editor, &mut paths);
    if paths.is_empty() {
        // Rune already gone: nothing left to port.
        return;
    }
    paths.sort();
    let inputs: Vec<Input> = paths
        .iter()
        .map(|p| Input {
            path: p.strip_prefix(&editor).unwrap_or(p).display().to_string(),
            source: std::fs::read_to_string(p).expect("readable"),
            key: None,
        })
        .collect();
    let api = std::fs::read_to_string(root.join("docs/generated/api.json"))
        .map_or_else(|_| Api::default(), |t| Api::from_json(&t));
    let ported = port_files(&inputs, api);
    assert_eq!(ported.len(), inputs.len());
    let (mut code, mut clean) = (0, 0);
    for p in &ported {
        assert!(p.bs.starts_with("// Ported from "), "{}", p.path);
        assert!(
            !p.bs.lines().any(|l| l.trim_start().starts_with("/*")),
            "{}: .bs has no block comments",
            p.path
        );
        assert!(p.report.flagged_lines <= p.report.code_lines, "{}", p.path);
        code += p.report.code_lines;
        clean += p.report.clean_lines();
    }
    // Most of the editor maps; a drop here is a regression worth a look.
    assert!(clean * 10 >= code * 8, "{clean} of {code} lines clean");
}
