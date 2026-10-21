// Porting files on disk: every `.rn` named or under a named directory, ported
// together so a call across files is typed from both ends, written beside
// the sources or under another directory.

use crate::{Api, Input, Ported, port_files};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Where the `.bs` files go.
pub enum Dest<'a> {
    /// Beside each `.rn`. A `.bs` already there is a person's work and is
    /// kept, unless `force`.
    Beside { force: bool },
    /// At each file's path under this directory, with a `.port.md` report
    /// beside each and `SUMMARY.md` at the top.
    Under(&'a Path),
}

/// What a port did.
#[derive(Default)]
pub struct Outcome {
    pub written: Vec<PathBuf>,
    /// `.bs` files already beside their `.rn`, left as they were.
    pub kept: Vec<PathBuf>,
    pub code_lines: usize,
    pub clean_lines: usize,
    /// Per file: its path and how many of its lines are marked.
    pub flagged: Vec<(String, usize)>,
}

impl Outcome {
    pub fn percent_clean(&self) -> f64 {
        pct(self.clean_lines, self.code_lines)
    }
}

/// Every `.rn` under `paths`, in a stable order.
pub fn rune_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = vec![];
    for p in paths {
        collect(p, &mut files);
    }
    files.sort();
    files
}

/// `docs/generated/api.json` in the first parent of `from` that has one.
pub fn find_api(from: &Path) -> Option<PathBuf> {
    let abs = std::fs::canonicalize(from).ok()?;
    abs.ancestors()
        .map(|a| a.join("docs/generated/api.json"))
        .find(|p| p.is_file())
}

/// The directory every file is under.
pub fn common_root(files: &[PathBuf]) -> PathBuf {
    let Some(first) = files.first() else {
        return PathBuf::new();
    };
    let mut root = first.parent().map(Path::to_path_buf).unwrap_or_default();
    for f in files {
        while !f.starts_with(&root) {
            if !root.pop() {
                return PathBuf::new();
            }
        }
    }
    root
}

/// The files read, each named by its path under their common root.
pub fn read_inputs(files: &[PathBuf]) -> Result<(PathBuf, Vec<Input>), String> {
    let root = common_root(files);
    let mut inputs = vec![];
    for f in files {
        let source = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let rel = f.strip_prefix(&root).unwrap_or(f);
        inputs.push(Input {
            path: rel.display().to_string(),
            source,
            key: None,
        });
    }
    Ok((root, inputs))
}

/// Port every `.rn` under `paths` and write the drafts to `dest`.
pub fn port_tree(paths: &[PathBuf], api: Api, dest: &Dest<'_>) -> Result<Outcome, String> {
    let files = rune_files(paths);
    if files.is_empty() {
        return Err("no .rn files in what was named".into());
    }
    let (root, inputs) = read_inputs(&files)?;
    let ported = port_files(&inputs, api);
    let mut out = Outcome::default();
    for p in &ported {
        let r = &p.report;
        out.code_lines += r.code_lines;
        out.clean_lines += r.clean_lines();
        out.flagged.push((p.path.clone(), r.flagged_lines));
    }
    match dest {
        Dest::Beside { force } => {
            for p in &ported {
                let bs = root.join(Path::new(&p.path).with_extension("bs"));
                if bs.exists() && !force {
                    out.kept.push(bs);
                    continue;
                }
                std::fs::write(&bs, &p.bs).map_err(|e| format!("{}: {e}", bs.display()))?;
                out.written.push(bs);
            }
        }
        Dest::Under(dir) => {
            for p in &ported {
                out.written.push(write_under(dir, p)?);
            }
            let summary = dir.join("SUMMARY.md");
            std::fs::write(&summary, summary_of(&ported))
                .map_err(|e| format!("{}: {e}", summary.display()))?;
        }
    }
    Ok(out)
}

// A file's `.bs` and its report under `dir`.
fn write_under(dir: &Path, p: &Ported) -> Result<PathBuf, String> {
    let dest = dir.join(Path::new(&p.path).with_extension("bs"));
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&dest, &p.bs).map_err(|e| format!("{}: {e}", dest.display()))?;
    let report = dir.join(Path::new(&p.path).with_extension("port.md"));
    std::fs::write(&report, p.report.markdown(&p.path))
        .map_err(|e| format!("{}: {e}", report.display()))?;
    Ok(dest)
}

/// Every file's clean share, and the flags by kind.
fn summary_of(ported: &[Ported]) -> String {
    let mut summary = String::from(
        "# balaur port summary\n\n| file | code lines | clean | flagged | clean % |\n| --- | ---: | ---: | ---: | ---: |\n",
    );
    let (mut code, mut clean) = (0usize, 0usize);
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for p in ported {
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
    summary
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
