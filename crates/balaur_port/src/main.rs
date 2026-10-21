//! `balaur-port <file.rn | dir>... --out <dir>`: Rune to Balaur Script.
//!
//! Every `.rn` named or under a named directory is ported together, so a call
//! across files is typed from both ends. Each lands at its path under `--out`
//! (relative to the common root) as `.bs`, with a `.port.md` report beside
//! it and `SUMMARY.md` at the top. Without `--out` the `.bs` goes to stdout
//! and the reports to stderr. `balaur port` is the same, with the engine's
//! own API rather than a file of it.

use balaur_port::files::{Dest, common_root, find_api, port_tree, read_inputs, rune_files};
use balaur_port::{Api, port_files};
use std::path::PathBuf;
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
    let files = rune_files(&paths);
    if files.is_empty() {
        eprintln!("balaur-port: no .rn files in what was named");
        return ExitCode::from(2);
    }
    let api_file = api_path.or_else(|| find_api(&common_root(&files)));
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
    let Some(out) = out else {
        let inputs = match read_inputs(&files) {
            Ok((_, inputs)) => inputs,
            Err(e) => {
                eprintln!("balaur-port: {e}");
                return ExitCode::FAILURE;
            }
        };
        for p in port_files(&inputs, api) {
            print!("{}", p.bs);
            eprint!("{}", p.report.markdown(&p.path));
        }
        return ExitCode::SUCCESS;
    };
    match port_tree(&paths, api, &Dest::Under(&out)) {
        Ok(done) => {
            eprintln!(
                "balaur-port: {} files, {} code lines, {} clean ({:.1}%), {} flagged; see {}",
                done.written.len(),
                done.code_lines,
                done.clean_lines,
                done.percent_clean(),
                done.code_lines - done.clean_lines,
                out.join("SUMMARY.md").display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("balaur-port: {e}");
            ExitCode::FAILURE
        }
    }
}
