// An export pack built through two backends: each script goes to the one its
// extension names, as `Mixed` does for a running game.

use crate::host::{BsHost, EXTENSION};
use balaur_core::Engine;
use balaur_script::ScriptCompiler;

/// `primary`'s compiler with `.bs` beside it, when `eng` runs both.
pub struct Exporter<'a> {
    primary: &'a dyn ScriptCompiler,
    lang: Option<BsHost>,
    extensions: Vec<&'a str>,
}

impl<'a> Exporter<'a> {
    pub fn new(eng: &Engine, primary: &'a dyn ScriptCompiler) -> Self {
        let lang = BsHost::of(eng);
        let mut extensions = primary.extensions().to_vec();
        if lang.is_some() {
            extensions.push(EXTENSION);
        }
        Exporter {
            primary,
            lang,
            extensions,
        }
    }
}

impl ScriptCompiler for Exporter<'_> {
    fn extensions(&self) -> &[&str] {
        &self.extensions
    }

    fn compile(&self, rel: &str, source: &str) -> anyhow::Result<Vec<u8>> {
        match &self.lang {
            Some(lang) if BsHost::claims(rel) => lang.compile(rel, source),
            _ => self.primary.compile(rel, source),
        }
    }

    fn uses(&self, rel: &str, source: &str) -> anyhow::Result<Vec<String>> {
        match &self.lang {
            Some(lang) if BsHost::claims(rel) => lang.uses(rel, source),
            _ => self.primary.uses(rel, source),
        }
    }

    fn api(&self) -> Vec<String> {
        self.lang
            .as_ref()
            .map(ScriptCompiler::api)
            .unwrap_or_default()
    }
}
