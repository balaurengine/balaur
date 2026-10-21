//! `balaur port`: a project's Rune scripts as Balaur Script drafts, typed
//! against this engine's own API rather than a file of it.

use anyhow::Result;
use balaur_port::files::{Dest, port_tree};
use std::path::Path;

pub(crate) fn run(path: &Path, out: Option<&Path>, force: bool) -> Result<()> {
    let api = balaur_port::Api::from_json(&crate::api_dump::api_value()?.to_string());
    let dest = match out {
        Some(dir) => Dest::Under(dir),
        None => Dest::Beside { force },
    };
    let done = port_tree(&[path.to_path_buf()], api, &dest).map_err(anyhow::Error::msg)?;
    for f in &done.written {
        println!("wrote {}", f.display());
    }
    for f in &done.kept {
        println!(
            "kept {}: it was there already; --force replaces it",
            f.display()
        );
    }
    println!(
        "{} code lines, {:.1}% clean; each line marked `{}` is left to finish",
        done.code_lines,
        done.percent_clean(),
        balaur_port::MARK
    );
    Ok(())
}
