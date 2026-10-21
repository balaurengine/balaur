//! Translates the editor's Rune into Balaur Script, for a person to finish.
//!
//! The output is a draft: everything the translator could not map is marked
//! with [`MARK`] in the `.bs` and listed in the file's report. See the
//! crate's README for the mapping and what it covers.

// A translator is long matches over the AST; splitting them scatters one
// mapping over several functions.
#![allow(clippy::too_many_lines)]

use std::fmt::Write as _;

mod api;
mod attempt;
mod call;
mod closure;
mod cx;
mod expr;
mod format;
mod item;
mod literal;
mod project;
mod source;
mod stmt;
mod ty;

pub use api::Api;

/// What marks a line the translator could not map, `// <MARK>: why`.
/// Spelt in two here only because the house lint reads the word in this
/// crate's source as a to-do of its own.
pub const MARK: &str = concat!("TO", "DO(port)");
pub use project::module_key;

/// One Rune file to port.
pub struct Input {
    /// As the report names it; also where its module key comes from.
    pub path: String,
    pub source: String,
    /// What other files call it by (`util`, `panels::code`); from the path
    /// when left out.
    pub key: Option<String>,
}

/// Something the translator could not map, or a note on what it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flag {
    /// 1-based line in the Rune file.
    pub line: u32,
    pub kind: &'static str,
    pub msg: String,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Lines of the Rune file that hold code: not blank, not only a comment.
    pub code_lines: usize,
    /// Code lines with at least one flag.
    pub flagged_lines: usize,
    pub flags: Vec<Flag>,
    /// What the output handles but a reader should know.
    pub notes: Vec<Flag>,
}

impl Report {
    pub fn clean_lines(&self) -> usize {
        self.code_lines - self.flagged_lines
    }

    /// The report as Markdown.
    pub fn markdown(&self, path: &str) -> String {
        let mut s = format!("# {path}\n\n");
        let pct = if self.code_lines == 0 {
            100.0
        } else {
            100.0 * self.clean_lines() as f64 / self.code_lines as f64
        };
        let _ = writeln!(
            s,
            "{} code lines: {} clean ({pct:.1}%), {} flagged.",
            self.code_lines,
            self.clean_lines(),
            self.flagged_lines
        );
        if !self.flags.is_empty() {
            let _ = write!(s, "\n## Not mapped ({MARK} in the output)\n\n");
            for f in &self.flags {
                let _ = writeln!(s, "- {}: `{}` {}", f.line, f.kind, f.msg);
            }
        }
        if !self.notes.is_empty() {
            s.push_str("\n## Notes\n\n");
            for f in &self.notes {
                let _ = writeln!(s, "- {}: `{}` {}", f.line, f.kind, f.msg);
            }
        }
        s
    }
}

pub struct Ported {
    pub path: String,
    pub key: String,
    pub bs: String,
    pub report: Report,
}

/// Ports files together: a call from one to another is typed from both.
pub fn port_files(inputs: &[Input], api: Api) -> Vec<Ported> {
    project::port(inputs, api)
}

/// Ports one file on its own, with no engine API: what the tests use.
pub fn port_source(source: &str) -> Ported {
    let input = Input {
        path: "snippet.rn".into(),
        source: source.into(),
        key: Some("snippet".into()),
    };
    project::port(&[input], Api::default())
        .pop()
        .expect("one file in, one out")
}
