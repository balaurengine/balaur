//! `balaur-port <file.rn | dir>... --out <dir>`: Rune to Balaur Script.
//!
//! Every `.rn` named or under a named directory is ported together, so a call
//! across files is typed from both ends. Each lands at its path under `--out`
//! (relative to the common root) as `.bs`, with a `.port.md` report beside
//! it and `SUMMARY.md` at the top. Without `--out` the `.bs` goes to stdout
//! and the reports to stderr.

use balaur_port::{Api, Input, Ported, port_files};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str =
    "usage: balaur-port <file.rn | dir>... [--out DIR] [--api docs/generated/api.json]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut paths = vec![];
    let mut out: Option<PathBuf> = None;
    let mut api_path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => out = args.next().map(PathBuf::from),
            "--api" => api_path = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                eprintln!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => paths.push(PathBuf::from(a)),
        }
    }
    if paths.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut files = vec![];
    for p in &paths {
        collect(p, &mut files);
    }
    files.sort();
    if files.is_empty() {
        eprintln!("balaur-port: no .rn files in what was named");
        return ExitCode::from(2);
    }
    let root = common_root(&files);
    let api_file = api_path.or_else(|| find_api(&root));
    let api = if let Some(text) = api_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
    {
        Api::from_json(&text)
    } else {
        eprintln!(
            "balaur-port: no docs/generated/api.json found; every `module::fn` is taken as the engine's"
        );
        Api::default()
    };
    let mut inputs = vec![];
    for f in &files {
        let source = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("balaur-port: {}: {e}", f.display());
                return ExitCode::FAILURE;
            }
        };
        let rel = f.strip_prefix(&root).unwrap_or(f);
        inputs.push(Input {
            path: rel.display().to_string(),
            source,
            key: None,
        });
    }
    let ported = port_files(&inputs, api);
    let Some(out) = out else {
        for p in ported {
            print!("{}", p.bs);
            eprint!("{}", p.report.markdown(&p.path));
        }
        return ExitCode::SUCCESS;
    };
    write_out(&ported, &out)
}

/// Each file's `.bs` and report under `out`, and `SUMMARY.md` over them.
fn write_out(ported: &[Ported], out: &Path) -> ExitCode {
    let mut summary = String::from(
        "# balaur-port summary\n\n| file | code lines | clean | flagged | clean % |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    let (mut code, mut clean) = (0usize, 0usize);
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for p in ported {
        let dest = out.join(Path::new(&p.path).with_extension("bs"));
        if let Some(dir) = dest.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&dest, &p.bs) {
            eprintln!("balaur-port: {}: {e}", dest.display());
            return ExitCode::FAILURE;
        }
        let report = out.join(Path::new(&p.path).with_extension("port.md"));
        if let Err(e) = std::fs::write(&report, p.report.markdown(&p.path)) {
            eprintln!("balaur-port: {}: {e}", report.display());
            return ExitCode::FAILURE;
        }
        let r = &p.report;
        code += r.code_lines;
        clean += r.clean_lines();
        for f in &r.flags {
            *kinds.entry(f.kind).or_default() += 1;
        }
        let _ = writeln!(
            summary,
            "| {} | {} | {} | {} | {:.1} |",
            p.path,
            r.code_lines,
            r.clean_lines(),
            r.flagged_lines,
            pct(r.clean_lines(), r.code_lines)
        );
    }
    let _ = writeln!(
        summary,
        "| **all {} files** | {code} | {clean} | {} | {:.1} |",
        ported.len(),
        code - clean,
        pct(clean, code)
    );
    summary.push_str("\n## Flags by kind\n\n| kind | count |\n| --- | ---: |\n");
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(k.1));
    for (k, n) in kinds {
        let _ = writeln!(summary, "| `{k}` | {n} |");
    }
    if let Err(e) = std::fs::write(out.join("SUMMARY.md"), &summary) {
        eprintln!("balaur-port: SUMMARY.md: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "balaur-port: {} files, {code} code lines, {clean} clean ({:.1}%), {} flagged; see {}",
        ported.len(),
        pct(clean, code),
        code - clean,
        out.join("SUMMARY.md").display()
    );
    ExitCode::SUCCESS
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 {
        100.0
    } else {
        100.0 * a as f64 / b as f64
    }
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            collect(&e.path(), out);
        }
    } else if p.extension().is_some_and(|e| e == "rn") {
        out.push(p.to_path_buf());
    }
}

fn common_root(files: &[PathBuf]) -> PathBuf {
    let mut root = files[0].parent().map(Path::to_path_buf).unwrap_or_default();
    for f in files {
        while !f.starts_with(&root) {
            if !root.pop() {
                return PathBuf::new();
            }
        }
    }
    root
}

/// `docs/generated/api.json` in the first parent that has one.
fn find_api(from: &Path) -> Option<PathBuf> {
    let abs = std::fs::canonicalize(from).ok()?;
    abs.ancestors()
        .map(|a| a.join("docs/generated/api.json"))
        .find(|p| p.is_file())
}
