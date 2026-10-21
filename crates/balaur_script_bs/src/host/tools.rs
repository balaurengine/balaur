// The editor's tooling on a `.bs` buffer, read against the engine's API.

use super::{BsHost, key_of};
use balaur_bs::analysis::{Analysis, Kind, analyze_in};
use balaur_script::{Diagnostic, Item, LanguageTools, Location, Symbol};

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Field => "field",
        Kind::Method => "method",
        Kind::Function => "function",
        Kind::Variable => "variable",
        Kind::Struct => "struct",
        Kind::Module => "module",
        Kind::Property => "property",
        Kind::Keyword => "keyword",
        Kind::Constant => "constant",
    }
}

fn at(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl BsHost {
    // The buffer as the file at `path`, its `use`s found from there.
    fn analysis(&self, path: &str, source: &str) -> Analysis {
        analyze_in(source, &self.api(), Some(self.modules()), &key_of(path))
    }
}

impl LanguageTools for BsHost {
    fn check(&self, path: &str, source: &str) -> Vec<Diagnostic> {
        self.check(path, source)
            .into_iter()
            .map(|e| Diagnostic {
                line: e.pos.line as usize,
                column: e.pos.col as usize,
                severity: "error",
                message: e.msg,
            })
            .collect()
    }

    fn complete(&self, path: &str, source: &str, line: usize, column: usize) -> Vec<Item> {
        self.analysis(path, source)
            .complete(at(line), at(column))
            .into_iter()
            .map(|c| Item {
                label: c.label,
                kind: kind_name(c.kind),
                detail: c.detail,
            })
            .collect()
    }

    fn hover(&self, path: &str, source: &str, line: usize, column: usize) -> Option<String> {
        self.analysis(path, source).hover(at(line), at(column))
    }

    fn definition(&self, path: &str, source: &str, line: usize, column: usize) -> Option<Location> {
        let p = self
            .analysis(path, source)
            .definition(at(line), at(column))?;
        Some(Location {
            file: key_of(path),
            line: p.line as usize,
            column: p.col as usize,
        })
    }

    fn symbols(&self, path: &str, source: &str) -> Vec<Symbol> {
        self.analysis(path, source)
            .symbols()
            .into_iter()
            .map(|s| Symbol {
                name: s.name,
                kind: kind_name(s.kind),
                detail: s.detail,
                line: s.pos.line as usize,
                column: s.pos.col as usize,
            })
            .collect()
    }

    fn signature(
        &self,
        path: &str,
        source: &str,
        line: usize,
        column: usize,
    ) -> Option<(String, usize)> {
        self.analysis(path, source).signature(at(line), at(column))
    }

    fn references(&self, path: &str, source: &str, name: &str) -> Vec<Location> {
        self.analysis(path, source)
            .references(name)
            .into_iter()
            .map(|p| Location {
                file: key_of(path),
                line: p.line as usize,
                column: p.col as usize,
            })
            .collect()
    }

    fn rename(&self, path: &str, source: &str, from: &str, to: &str) -> Option<String> {
        self.analysis(path, source).rename(from, to)
    }

    fn format(&self, path: &str, source: &str) -> Option<String> {
        Some(self.analysis(path, source).format())
    }
}
